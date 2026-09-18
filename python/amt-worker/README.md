# AMT worker

Optional, isolated local Basic Pitch worker. It deliberately uses CPython 3.10
on Windows: Basic Pitch 0.4 selects its bundled ONNX model and ONNX Runtime on
that interpreter, avoiding the unsupported TensorFlow-IO dependency path used
by Python 3.11. Install deliberately with `uv sync` in this directory. The
desktop always starts it with `--offline` and never downloads a runtime or
model while the application is running.
