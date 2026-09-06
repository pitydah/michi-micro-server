# Michi Micro Server — Release Gate

El estado vivo del release no se versiona en este archivo.

La evidencia autoritativa se genera por GitHub Actions
para el SHA evaluado y se publica como:

- artifact `release-gate-report-<sha>`
- GitHub Actions Step Summary

Evaluación local:

```bash
python3 scripts/generate_release_gate.py \
  --mode rc \
  --evidence-dir target/release-evidence \
  --report target/V1_RELEASE_GATE.md \
  --check
```
