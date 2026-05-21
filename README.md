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

### Example Config

To use OpenCode Zen or NVidia, save into `~/.nidex/config.toml`. Set the envvars, OPENAI_API_KEY etc to your LLM provider's API key.  

```toml
sandbox_mode = "danger-full-access"

[analytics]
enabled = false

[profiles.opencode]
model_provider = "opencode"
model_reasoning_effort = "none"

[model_providers.opencode]
name = "opencode"
base_url = "https://opencode.ai/zen/v1"
env_key = "OPENAI_API_KEY"
wire_api = "chat"

[profiles.nvidia]
model_provider = "nvidia"
model = "deepseek-ai/deepseek-v4-flash"

[model_providers.nvidia]
name = "nvidia"
base_url = "https://integrate.api.nvidia.com/v1"
env_key = "NVIDIA_API_KEY"
wire_api = "chat"

[features]
steer = true
apply_patch_json = true
apply_patch_tool = true
collaboration_modes = true
```

Then use like this:

```bash
nixdex -p opencode
```
