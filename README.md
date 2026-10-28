# noname

A terminal-based coding agent that runs locally against [Ollama](https://ollama.com).

## Build

```bash
cd codex-rs
cargo build -p codex-cli
```

## Run

```bash
./target/debug/noname --no-daemon
```

Make sure Ollama is running (`ollama serve`) with at least one model pulled.

## License

Apache-2.0