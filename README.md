# Aevum

<p>
  <img src="./logo.svg" width="10%" align="left">

  Aevum is a mobile-optimized Wayland alarm clock which automatically handles
  suspend and reboot with optional logind support.

  <br clear="align"/>
</p>

<br />

## Screenshots

<p align="center">
  <img src="https://github.com/user-attachments/assets/830e06af-1420-41db-93fe-627f42c7af7c" width="30%"/>
  <img src="https://github.com/user-attachments/assets/3712883f-c1d6-4050-8927-85b28a5e793d" width="30%"/>
</p>

## Building from Source

Aevum is compiled with cargo, which creates a binary at `target/release/aevum`:

```sh
cargo build --release
```

### CLI Examples

List all pending alarms:

```
$ aevum-cli list
ID                                    Alarm Time
45ecd456-e151-4942-917f-58c953213edf  Wed, 10 Sep 2025 16:00:00 +0200
```

Create a new alarm at `16:00`:

```
$ aevum-cli add 16:00
Added alarm with ID "45ecd456-e151-4942-917f-58c953213edf"
```

Delete an alarm:

```
$ aevum-cli remove 45ecd456-e151-4942-917f-58c953213edf
Removed alarm with ID ["45ecd456-e151-4942-917f-58c953213edf"]
```
