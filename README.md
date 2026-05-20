<p align="center"><strong>Nidex CLI</strong> is a coding agent forked from OpenAI that runs locally on your computer.
<p align="center">
  <img src=".github/codex-cli-splash.png" alt="Nidex CLI splash" width="80%" />
</p>
</br>

---

## What is this...

This is Nidex, an agentic coding harness for the Knights That Say Ni. A "fork" if you like of OpenAI's Codex. OpenAI removed the older `chat/completions` API support from the official 
Codex agent. So I forked and added that back in while removing unecessary features. Codex itself is well written, fast and efficient. By also having control of the agentic harness 
you have control over the system prompt.

## Quickstart

### Installing and running Nidex CLI

Git clone and then build and run using the Rust Cargo toolchain.

```shell
cd codex-rs
cargo build --release
./target/relese/nidex
```

## Docs

This repository is licensed under the [Apache-2.0 License](LICENSE).
