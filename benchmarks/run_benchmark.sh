#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "=============================================="
echo "  pdfer_forms Benchmark Suite"
echo "=============================================="
echo ""

# 1. Run Python baseline
echo ">>> Step 1/3: Running pypdf / PyPDF2 baseline..."
if [ ! -d ".venv" ]; then
    python3 -m venv .venv
    source .venv/bin/activate
    pip install -q pypdf PyPDF2 cryptography
else
    source .venv/bin/activate
fi
python3 pypdf_baseline.py
echo ""

# 2. Build and run Rust benchmark
echo ">>> Step 2/3: Building and running pdfer_forms benchmark..."
cargo run --release 2>&1
echo ""

# 3. Compare results
echo ">>> Step 3/3: Generating comparison report..."
echo ""
python3 compare_results.py
