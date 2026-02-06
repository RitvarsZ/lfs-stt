---

# LFS-STT - Live for Speed speech to text

lfs-stt is a **InSim plugin** for **Live For Speed** that enables **speech-to-text chat** using **OpenAI’s Whisper model**.
Players can record audio in-game, have it transcribed, and send messages directly into the server chat. Multiple **chat channels** can be configured with custom prefixes — especially useful on cruise servers or organized events.

![](img/demo.gif)
---

## Features

* Record messages in-game via configurable binds
* Speech-to-text using Whisper
* Message preview before sending
* Cycle through multiple chat channels (message prefixes)
* Configurable UI position, scale, and timing settings
* Optional GPU acceleration for faster transcription (highly recommended as running on CPU is very slow)

---

## In-Game Setup

Set up the following **InSim binds** in LFS:

| Command         | Description                                                                        |
| --------------- | ---------------------------------------------------------------------------------- |
| `/o stt talk`   | Toggle recording on/off                                                            |
| `/o stt accept` | Accept the message in preview and send it to the server on the selected channel    |
| `/o stt nc`     | Select the next chat channel (cycles back to the first channel after the last one) |
| `/o stt pc`     | Select the previous chat channel                                                   |

---

## Configuration

All plugin settings are managed via a **TOML configuration file** (`config.toml`).

### Example `config.toml`

```toml
# ================================
# InSim connection settings
# ================================

# InSim host
insim_host = "127.0.0.1"

# InSim port
insim_port = "29999"

# ================================
# Model / AI settings
# ================================

# Path to the whisper model directory or file
# By default the plugin ships with a small English-only model, which is sufficient for most use cases.
# Download other models from https://huggingface.co/ggerganov/whisper.cpp
model_path = "models/small.en.bin"

# Whether to use GPU acceleration to run the speech-to-text model
# Requires Nvidia GPU and CUDA installed
use_gpu = false

# ================================
# Timing settings
# ================================

# How long message previews stay visible
message_preview_timeout_secs = 20

# Maximum message recording duration
recording_timeout_secs = 10

# ================================
# UI layout settings
# ================================

# UI scale factor
ui_scale = 5

# Vertical UI offset (0–200)
ui_offset_top = 170

# Horizontal UI offset (0–200)
ui_offset_left = 10

# Button ID offset (0–230)
# Use if buttons are conflicting with other InSim plugins
btn_id_offset = 50

# ================================
# Advanced settings
# ================================

# When true, last recorded message is saved to debug.wav
debug_audio_resampling = false

# Logging verbosity
# Valid values: error, warn, info, debug, trace
debug_log_level = "info"

# ================================
# Chat channels
# ================================
# You must define at least ONE chat channel.
# Each channel needs a non-empty display name.
# Add as many channels as you want.

[[chat_channels]]
# What you see in the UI
display = "/say"
# Message prefix sent to InSim
prefix = ""

[[chat_channels]]
display = "^5!local"
prefix = "!l"
```

---

## Usage

1. Download the latest [release](https://github.com/RitvarsZ/lfs-stt/releases)
2. Configure InSim binds in LFS as described above.
3. Launch LFS and launch `lfs-stt.exe`.
4. Press your `talk` bind to start recording, press it again to stop. Press `accept` to send the transcribed result.
5. Use `nc` / `pc` binds to switch between chat channels.

---

## Tips

* **Default channels:** The plugin comes with `/say` and `!local` configured by default, but you can change them or add more by adding more `[[chat_channels]]` blocks.
* **GPU usage:** Enable `use_gpu = true` only if your system supports it — otherwise CPU works fine.
* **UI customization:** Adjust `ui_scale`, `ui_offset_top`, and `ui_offset_left` to avoid overlapping with other InSim plugins.
* **Logging:** `debug_log_level` can help troubleshoot issues — set to `debug` or `trace` during testing.

---

## Contributing

* PRs and bug reports welcome.
* Disclaimer - I'm using this project as an opportunity to learn Rust.

---

