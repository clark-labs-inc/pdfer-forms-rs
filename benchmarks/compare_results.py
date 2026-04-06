#!/usr/bin/env python3
"""
Compare pdfer_forms (Rust) results against pypdf/PyPDF2 baseline.

Reads rust_benchmark.json and pypdf_baseline.json, then produces a
consolidated report covering:
  1. Accuracy  – field extraction agreement
  2. Performance – timing comparisons
  3. Functionality – API coverage / parity
"""

import json
import sys
from pathlib import Path


def load_json(path):
    with open(path) as f:
        return json.load(f)


def format_ms(ms):
    if ms is None:
        return "N/A"
    if ms < 1:
        return f"{ms:.3f}ms"
    if ms < 100:
        return f"{ms:.1f}ms"
    return f"{ms:.0f}ms"


def speedup(baseline_ms, rust_ms):
    if baseline_ms is None or rust_ms is None or rust_ms == 0:
        return "N/A"
    ratio = baseline_ms / rust_ms
    if ratio > 1:
        return f"{ratio:.1f}x faster"
    else:
        return f"{1/ratio:.1f}x slower"


def compare_field_names(pypdf_fields, rust_fields):
    """Compare field names between pypdf and pdfer_forms."""
    py_names = set(pypdf_fields.keys()) if pypdf_fields else set()
    rs_names = set(rust_fields.keys()) if rust_fields else set()

    common = py_names & rs_names
    py_only = py_names - rs_names
    rs_only = rs_names - py_names

    return {
        "common": len(common),
        "pypdf_only": sorted(py_only),
        "rust_only": sorted(rs_only),
        "match_pct": (len(common) / max(len(py_names), 1)) * 100,
    }


def compare_field_types(pypdf_fields, rust_fields):
    """Compare field types for commonly named fields."""
    if not pypdf_fields or not rust_fields:
        return {"matches": 0, "mismatches": 0, "details": []}

    matches = 0
    mismatches = 0
    details = []
    for name in sorted(set(pypdf_fields.keys()) & set(rust_fields.keys())):
        py_type = pypdf_fields[name].get("field_type")
        rs_type = rust_fields[name].get("field_type")
        if py_type == rs_type:
            matches += 1
        else:
            mismatches += 1
            details.append(f"  {name}: pypdf={py_type} vs rust={rs_type}")

    return {"matches": matches, "mismatches": mismatches, "details": details}


def compare_field_values(pypdf_fields, rust_fields):
    """Compare field values for commonly named fields."""
    if not pypdf_fields or not rust_fields:
        return {"matches": 0, "mismatches": 0, "details": []}

    matches = 0
    mismatches = 0
    details = []
    for name in sorted(set(pypdf_fields.keys()) & set(rust_fields.keys())):
        py_val = pypdf_fields[name].get("value")
        rs_val = rust_fields[name].get("value")
        # Normalize None vs empty string
        py_norm = py_val if py_val else None
        rs_norm = rs_val if rs_val else None
        if py_norm == rs_norm:
            matches += 1
        else:
            mismatches += 1
            if len(details) < 10:
                details.append(f"  {name}: pypdf={py_val!r} vs rust={rs_val!r}")

    return {"matches": matches, "mismatches": mismatches, "details": details}


def main():
    bench_dir = Path(__file__).parent

    rust_path = bench_dir / "rust_benchmark.json"
    pypdf_path = bench_dir / "pypdf_baseline.json"

    if not rust_path.exists():
        print(f"ERROR: {rust_path} not found. Run the Rust benchmark first.")
        sys.exit(1)
    if not pypdf_path.exists():
        print(f"ERROR: {pypdf_path} not found. Run pypdf_baseline.py first.")
        sys.exit(1)

    rust_data = load_json(rust_path)
    pypdf_data = load_json(pypdf_path)
    pypdf_results = pypdf_data["pypdf"]

    # ======================================================================
    # Header
    # ======================================================================
    print("=" * 80)
    print("  pdfer_forms vs pypdf/PyPDF2 — Benchmark Report")
    print("=" * 80)
    print()
    print(f"  Rust library:  pdfer_forms v{rust_data['version']}")
    print(f"  pypdf:         v{pypdf_results['version']}")
    print(f"  PyPDF2:        v{pypdf_data['PyPDF2']['version']}")
    print()

    # ======================================================================
    # 1. ACCURACY — Field Extraction
    # ======================================================================
    print("=" * 80)
    print("  1. ACCURACY — Field Extraction Comparison")
    print("=" * 80)
    print()

    total_accuracy = {"name_match": 0, "name_total": 0, "type_match": 0,
                      "type_total": 0, "value_match": 0, "value_total": 0}

    for rel_path in sorted(set(rust_data["pdfs"].keys()) | set(pypdf_results["pdfs"].keys())):
        rust_pdf = rust_data["pdfs"].get(rel_path, {})
        py_pdf = pypdf_results["pdfs"].get(rel_path, {})

        print(f"  --- {rel_path} ---")

        rust_fields = rust_pdf.get("fields", {})
        py_fields = py_pdf.get("fields", {})

        # Field count
        rust_count = rust_pdf.get("field_count", 0)
        py_count = py_pdf.get("field_count", 0)
        print(f"  Field count:  pypdf={py_count}  rust={rust_count}")

        # Field name matching
        name_cmp = compare_field_names(py_fields, rust_fields)
        print(f"  Name match:   {name_cmp['match_pct']:.0f}% ({name_cmp['common']} common)")
        total_accuracy["name_match"] += name_cmp["common"]
        total_accuracy["name_total"] += max(len(py_fields) if py_fields else 0, 1)
        if name_cmp["pypdf_only"]:
            shown = name_cmp["pypdf_only"][:5]
            print(f"    pypdf-only: {shown}{'...' if len(name_cmp['pypdf_only']) > 5 else ''}")
        if name_cmp["rust_only"]:
            shown = name_cmp["rust_only"][:5]
            print(f"    rust-only:  {shown}{'...' if len(name_cmp['rust_only']) > 5 else ''}")

        # Field type matching
        type_cmp = compare_field_types(py_fields, rust_fields)
        total_accuracy["type_match"] += type_cmp["matches"]
        total_accuracy["type_total"] += type_cmp["matches"] + type_cmp["mismatches"]
        if type_cmp["mismatches"]:
            print(f"  Type mismatches: {type_cmp['mismatches']}")
            for d in type_cmp["details"][:3]:
                print(f"  {d}")

        # Field value matching
        val_cmp = compare_field_values(py_fields, rust_fields)
        total_accuracy["value_match"] += val_cmp["matches"]
        total_accuracy["value_total"] += val_cmp["matches"] + val_cmp["mismatches"]
        if val_cmp["mismatches"]:
            print(f"  Value mismatches: {val_cmp['mismatches']}")
            for d in val_cmp["details"][:3]:
                print(f"  {d}")

        # Errors
        rust_errors = rust_pdf.get("errors", [])
        py_errors = py_pdf.get("errors", [])
        if rust_errors:
            print(f"  Rust errors: {rust_errors}")
        if py_errors:
            print(f"  pypdf errors: {py_errors}")

        print()

    # Accuracy summary
    print("  --- ACCURACY SUMMARY ---")
    if total_accuracy["name_total"]:
        print(f"  Field name match rate:  {total_accuracy['name_match']}/{total_accuracy['name_total']} "
              f"({total_accuracy['name_match']/total_accuracy['name_total']*100:.1f}%)")
    if total_accuracy["type_total"]:
        print(f"  Field type match rate:  {total_accuracy['type_match']}/{total_accuracy['type_total']} "
              f"({total_accuracy['type_match']/total_accuracy['type_total']*100:.1f}%)")
    if total_accuracy["value_total"]:
        print(f"  Field value match rate: {total_accuracy['value_match']}/{total_accuracy['value_total']} "
              f"({total_accuracy['value_match']/total_accuracy['value_total']*100:.1f}%)")
    print()

    # ======================================================================
    # 2. PERFORMANCE — Timing Comparison
    # ======================================================================
    print("=" * 80)
    print("  2. PERFORMANCE — Timing Comparison (pypdf vs pdfer_forms)")
    print("=" * 80)
    print()

    operations = ["load", "get_fields", "get_form_text_fields",
                   "get_pages_showing_field", "fill_form", "remove_annotations"]

    # Header
    print(f"  {'PDF':<30} {'Operation':<25} {'pypdf':>10} {'Rust':>10} {'Speedup':>15}")
    print(f"  {'-'*30} {'-'*25} {'-'*10} {'-'*10} {'-'*15}")

    perf_totals = {op: {"pypdf": 0.0, "rust": 0.0, "count": 0} for op in operations}

    for rel_path in sorted(rust_data["pdfs"].keys()):
        rust_pdf = rust_data["pdfs"].get(rel_path, {})
        py_pdf = pypdf_results["pdfs"].get(rel_path, {})
        rust_timings = rust_pdf.get("timings_ms", {})
        py_timings = py_pdf.get("timings_ms", {})

        short_name = rel_path.split("/")[-1][:28]
        first = True

        for op in operations:
            py_ms = py_timings.get(op)
            rs_ms = rust_timings.get(op)

            if py_ms is not None or rs_ms is not None:
                name_col = short_name if first else ""
                first = False
                sp = speedup(py_ms, rs_ms)
                print(f"  {name_col:<30} {op:<25} {format_ms(py_ms):>10} {format_ms(rs_ms):>10} {sp:>15}")

                if py_ms is not None and rs_ms is not None:
                    perf_totals[op]["pypdf"] += py_ms
                    perf_totals[op]["rust"] += rs_ms
                    perf_totals[op]["count"] += 1

        if not first:
            print()

    # Performance summary
    print(f"  {'--- AVERAGES ---':<30} {'':25} {'pypdf':>10} {'Rust':>10} {'Speedup':>15}")
    print(f"  {'-'*30} {'-'*25} {'-'*10} {'-'*10} {'-'*15}")
    for op in operations:
        t = perf_totals[op]
        if t["count"] > 0:
            avg_py = t["pypdf"] / t["count"]
            avg_rs = t["rust"] / t["count"]
            sp = speedup(avg_py, avg_rs)
            print(f"  {'':30} {op:<25} {format_ms(avg_py):>10} {format_ms(avg_rs):>10} {sp:>15}")
    print()

    # ======================================================================
    # 3. FUNCTIONALITY — API Coverage / Parity
    # ======================================================================
    print("=" * 80)
    print("  3. FUNCTIONALITY — API Parity Check")
    print("=" * 80)
    print()

    api_ops = {
        "load (PdfReader)": "load",
        "get_fields()": "get_fields",
        "get_form_text_fields()": "get_form_text_fields",
        "get_pages_showing_field()": "get_pages_showing_field",
        "update_page_form_field_values()": "fill_form",
        "remove_annotations()": "remove_annotations",
    }

    print(f"  {'pypdf API':<40} {'pdfer_forms Status':<20} {'PDFs Tested':>12}")
    print(f"  {'-'*40} {'-'*20} {'-'*12}")

    for api_name, op_key in api_ops.items():
        tested = 0
        passed = 0
        failed = 0
        for rel_path, rust_pdf in rust_data["pdfs"].items():
            timings = rust_pdf.get("timings_ms", {})
            errors = rust_pdf.get("errors", [])
            if op_key in timings:
                tested += 1
                has_error = any(op_key.replace("fill_form", "fill") in e for e in errors)
                if has_error:
                    failed += 1
                else:
                    passed += 1

        if tested > 0:
            status = "OK" if failed == 0 else f"PARTIAL ({passed}/{tested})"
        else:
            status = "NOT TESTED"

        print(f"  {api_name:<40} {status:<20} {tested:>12}")

    # PyPDF2 compatibility shims
    print()
    print("  PyPDF2 Compatibility Shims (camelCase aliases):")
    shims = [
        ("getFields()", "get_fields()"),
        ("getFormTextFields()", "get_form_text_fields()"),
        ("getPagesShowingField()", "get_pages_showing_field()"),
        ("setNeedAppearancesWriter()", "set_need_appearances_writer()"),
        ("updatePageFormFieldValues()", "update_page_form_field_values()"),
        ("addFormTopname()", "add_form_topname()"),
        ("renameFormTopname()", "rename_form_topname()"),
        ("reattachFields()", "reattach_fields()"),
    ]
    for camel, snake in shims:
        print(f"    {camel:<35} -> {snake:<35} [alias present]")

    print()

    # ======================================================================
    # 4. FORM FILLING VERIFICATION
    # ======================================================================
    print("=" * 80)
    print("  4. FORM FILLING — Write/Read-back Verification")
    print("=" * 80)
    print()

    for rel_path in sorted(rust_data["pdfs"].keys()):
        rust_pdf = rust_data["pdfs"].get(rel_path, {})
        fill_input = rust_pdf.get("fill_input", {})
        fill_readback = rust_pdf.get("fill_readback", {})

        if not fill_input:
            continue

        short_name = rel_path.split("/")[-1]
        verified = 0
        total = len(fill_input)

        for field_name, expected in fill_input.items():
            actual = fill_readback.get(field_name)
            if actual == expected:
                verified += 1

        status = "PASS" if verified == total else f"PARTIAL ({verified}/{total})"
        print(f"  {short_name:<35} {status}")

    print()
    print("=" * 80)
    print("  Benchmark complete.")
    print("=" * 80)


if __name__ == "__main__":
    main()
