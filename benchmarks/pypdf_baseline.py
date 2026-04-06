#!/usr/bin/env python3
"""
pypdf / PyPDF2 baseline benchmark.

Exercises every form-manipulation API that pdfer_forms_rs aims to replace,
captures timing and field-extraction results, and writes a JSON report
that the Rust benchmark can compare against.
"""

import json
import os
import sys
import time
import tempfile
import traceback
from pathlib import Path

# ---------------------------------------------------------------------------
# Import both libraries so we can benchmark each
# ---------------------------------------------------------------------------
import pypdf
from pypdf import PdfReader as PypdfReader, PdfWriter as PypdfWriter

import PyPDF2
from PyPDF2 import PdfReader as Pypdf2Reader, PdfWriter as Pypdf2Writer

PDF_DIR = Path(__file__).parent / "pdfs"

# Collect all PDFs with form fields
PDF_FILES = sorted(
    str(p)
    for lang in ("en", "es", "zh")
    for p in (PDF_DIR / lang).glob("*.pdf")
)


def _rel(path: str) -> str:
    return os.path.relpath(path, PDF_DIR)


# ---------------------------------------------------------------------------
# Timing helper
# ---------------------------------------------------------------------------
class Timer:
    def __init__(self):
        self.start = None
        self.elapsed_ms = None

    def __enter__(self):
        self.start = time.perf_counter()
        return self

    def __exit__(self, *_):
        self.elapsed_ms = (time.perf_counter() - self.start) * 1000


# ---------------------------------------------------------------------------
# Individual operation benchmarks
# ---------------------------------------------------------------------------

def bench_load(path: str, reader_cls):
    with Timer() as t:
        reader = reader_cls(path)
    return reader, t.elapsed_ms


def bench_get_fields(reader):
    with Timer() as t:
        fields = reader.get_fields()
    return fields, t.elapsed_ms


def bench_get_form_text_fields(reader):
    """pypdf exposes get_form_text_fields()."""
    with Timer() as t:
        try:
            text_fields = reader.get_form_text_fields()
        except Exception:
            text_fields = {}
    return text_fields, t.elapsed_ms


def bench_get_pages_for_field(reader, field_name):
    """pypdf PdfReader has no direct get_pages_showing_field,
    but we can iterate pages and look for /Annots referencing the field."""
    with Timer() as t:
        pages = []
        for i, page in enumerate(reader.pages):
            annots = page.get("/Annots")
            if annots:
                try:
                    annot_list = annots.get_object() if hasattr(annots, 'get_object') else annots
                    if not isinstance(annot_list, list):
                        continue
                    for annot in annot_list:
                        annot_obj = annot.get_object() if hasattr(annot, 'get_object') else annot
                        if isinstance(annot_obj, dict):
                            t_val = annot_obj.get("/T")
                            if t_val and str(t_val) == field_name:
                                pages.append(i)
                except Exception:
                    pass
    return pages, t.elapsed_ms


def bench_fill_form(path: str, reader_cls, writer_cls, fields_to_fill: dict):
    """Fill form fields and save to a temp file."""
    reader = reader_cls(path)
    with Timer() as t:
        writer = writer_cls()
        writer.append_pages_from_reader(reader)
        for page_num in range(len(reader.pages)):
            try:
                writer.update_page_form_field_values(
                    writer.pages[page_num], fields_to_fill
                )
            except Exception:
                pass
        with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as tmp:
            tmp_path = tmp.name
            writer.write(tmp)
    elapsed = t.elapsed_ms

    # Verify the filled fields by reading back
    verify_reader = reader_cls(tmp_path)
    verify_fields = verify_reader.get_fields() or {}
    filled_back = {}
    for k, v in verify_fields.items():
        val = v.get("/V")
        if val is not None:
            filled_back[k] = str(val)

    os.unlink(tmp_path)
    return filled_back, elapsed


def bench_remove_annotations(path: str, reader_cls, writer_cls):
    """Remove /Widget annotations."""
    reader = reader_cls(path)
    with Timer() as t:
        writer = writer_cls()
        for page in reader.pages:
            annots = page.get("/Annots")
            if annots:
                filtered = []
                for annot in annots:
                    annot_obj = annot.get_object()
                    subtype = annot_obj.get("/Subtype")
                    if str(subtype) != "/Widget":
                        filtered.append(annot)
                if filtered:
                    page[pypdf.generic.NameObject("/Annots")] = pypdf.generic.ArrayObject(filtered)
                else:
                    del page["/Annots"]
            writer.add_page(page)
        with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False) as tmp:
            tmp_path = tmp.name
            writer.write(tmp)
    elapsed = t.elapsed_ms
    os.unlink(tmp_path)
    return elapsed


# ---------------------------------------------------------------------------
# Serialize field data for comparison
# ---------------------------------------------------------------------------

def serialize_fields(fields: dict | None) -> dict:
    """Convert pypdf field dict to a serializable format."""
    if not fields:
        return {}
    result = {}
    for name, field in fields.items():
        ft = field.get("/FT")
        v = field.get("/V")
        dv = field.get("/DV")
        ff = field.get("/Ff", 0)

        entry = {
            "field_type": str(ft) if ft else None,
            "value": str(v) if v is not None else None,
            "default_value": str(dv) if dv is not None else None,
            "flags": int(ff) if ff else 0,
        }

        # States for buttons
        if ft and str(ft) == "/Btn":
            ap = field.get("/AP")
            if ap:
                n = ap.get("/N")
                if n and hasattr(n, "keys"):
                    entry["states"] = sorted(str(k) for k in n.keys())

        result[name] = entry
    return result


def serialize_text_fields(text_fields: dict | None) -> dict:
    if not text_fields:
        return {}
    return {k: (str(v) if v is not None else None) for k, v in text_fields.items()}


# ---------------------------------------------------------------------------
# Run full benchmark for one library
# ---------------------------------------------------------------------------

def run_benchmark(lib_name, reader_cls, writer_cls):
    results = {
        "library": lib_name,
        "version": pypdf.__version__ if lib_name == "pypdf" else PyPDF2.__version__,
        "pdfs": {},
    }

    for pdf_path in PDF_FILES:
        rel = _rel(pdf_path)
        print(f"  [{lib_name}] {rel} ... ", end="", flush=True)

        pdf_result = {
            "path": rel,
            "timings_ms": {},
            "field_count": 0,
            "text_field_count": 0,
            "errors": [],
        }

        try:
            # 1. Load
            reader, load_ms = bench_load(pdf_path, reader_cls)
            pdf_result["timings_ms"]["load"] = round(load_ms, 3)
            pdf_result["pages"] = len(reader.pages)

            # 2. get_fields
            fields, gf_ms = bench_get_fields(reader)
            pdf_result["timings_ms"]["get_fields"] = round(gf_ms, 3)
            pdf_result["field_count"] = len(fields) if fields else 0
            pdf_result["fields"] = serialize_fields(fields)

            # 3. get_form_text_fields
            text_fields, gtf_ms = bench_get_form_text_fields(reader)
            pdf_result["timings_ms"]["get_form_text_fields"] = round(gtf_ms, 3)
            pdf_result["text_field_count"] = len(text_fields) if text_fields else 0
            pdf_result["text_fields"] = serialize_text_fields(text_fields)

            # 4. get_pages_showing_field (use first text field)
            if fields:
                first_text_field = None
                for name, field in fields.items():
                    ft = field.get("/FT")
                    if ft and str(ft) == "/Tx":
                        first_text_field = name
                        break
                if first_text_field:
                    pages, gpf_ms = bench_get_pages_for_field(reader, first_text_field)
                    pdf_result["timings_ms"]["get_pages_showing_field"] = round(gpf_ms, 3)
                    pdf_result["pages_for_first_field"] = pages

            # 5. Fill form (fill first 3 text fields with test data)
            if fields:
                fill_data = {}
                count = 0
                for name, field in fields.items():
                    ft = field.get("/FT")
                    if ft and str(ft) == "/Tx" and count < 3:
                        fill_data[name] = f"TEST_{count}"
                        count += 1
                if fill_data:
                    filled_back, fill_ms = bench_fill_form(
                        pdf_path, reader_cls, writer_cls, fill_data
                    )
                    pdf_result["timings_ms"]["fill_form"] = round(fill_ms, 3)
                    pdf_result["fill_input"] = fill_data
                    pdf_result["fill_readback"] = filled_back

            # 6. Remove annotations
            try:
                ra_ms = bench_remove_annotations(pdf_path, reader_cls, writer_cls)
                pdf_result["timings_ms"]["remove_annotations"] = round(ra_ms, 3)
            except Exception as e:
                pdf_result["errors"].append(f"remove_annotations: {e}")

            print(f"{pdf_result['field_count']} fields, "
                  f"load={load_ms:.1f}ms, get_fields={gf_ms:.1f}ms")

        except Exception as e:
            pdf_result["errors"].append(str(e))
            traceback.print_exc()
            print(f"ERROR: {e}")

        results["pdfs"][rel] = pdf_result

    return results


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    print("=" * 70)
    print("pypdf / PyPDF2 Baseline Benchmark")
    print("=" * 70)
    print(f"PDF directory: {PDF_DIR}")
    print(f"PDFs found: {len(PDF_FILES)}")
    print()

    # Run pypdf benchmark
    print("--- pypdf ---")
    pypdf_results = run_benchmark("pypdf", PypdfReader, PypdfWriter)
    print()

    # Run PyPDF2 benchmark
    print("--- PyPDF2 ---")
    pypdf2_results = run_benchmark("PyPDF2", Pypdf2Reader, Pypdf2Writer)
    print()

    # Combine results
    combined = {
        "pypdf": pypdf_results,
        "PyPDF2": pypdf2_results,
    }

    output_path = Path(__file__).parent / "pypdf_baseline.json"
    with open(output_path, "w") as f:
        json.dump(combined, f, indent=2, default=str)

    print(f"Results written to {output_path}")


if __name__ == "__main__":
    main()
