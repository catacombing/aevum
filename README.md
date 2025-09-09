# Aevum

<p>
  <img src="./logo.svg" width="10%" align="left">

  Aevum is an OpenGL alarm clock UI based on [Rezz](../rezz), which automatically
  handles suspend and reboot with optional logind support.

  <br clear="align"/>
</p>

<br />

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
