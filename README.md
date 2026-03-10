# cosmic-ext-applet-now-playing

A small COSMIC panel applet that shows what is currently playing via MPRIS.

It displays:
- Current track title and artist in the panel
- A popup with album art and media controls
- Album-color inspired panel button styling

![screenshot of the applet](./res/screenshot-1.png)
![screenshot of the applet 2](./res/screenshot-2.png)

## Build

```bash
just build-release
```

## Run (Local)

```bash
just run
```

## Install (System-Wide)

Build and install using the provided `just` recipes:

```bash
just build-release
sudo just install
```

To rebuild and reinstall after code changes:

```bash
just build-release && sudo just install
```

For a fully clean rebuild:

```bash
just clean && just build-release && sudo just install
```

Then add the applet from COSMIC panel settings.

## Feedback

Feedback is very welcome.

If you report an issue, please include:
- COSMIC version
- Distro and kernel
- Player app used
- Steps to reproduce
- Expected vs actual behavior

## License

This project is licensed under the [GPL-3.0-only license](./LICENSE)
