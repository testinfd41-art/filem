# Filem Transfers for Noctalia

This Noctalia 5 plugin adds a bar widget for active Filem transfers. Clicking it
opens a panel with live progress, transfer routes, sizes, and speeds.

The widget reads snapshots written by Filem to
`$XDG_CACHE_HOME/filem/noctalia` (or `~/.cache/filem/noctalia` when
`XDG_CACHE_HOME` is unset). Snapshots are private to the current user and are
removed when Filem exits; the plugin ignores stale snapshots after five seconds
if Filem terminates unexpectedly.

For local development, copy this directory to
`$XDG_DATA_HOME/noctalia/plugins/filem-transfers` (or
`~/.local/share/noctalia/plugins/filem-transfers` when `XDG_DATA_HOME` is
unset), then enable **Filem Transfers** in Noctalia's plugin settings and add
the `testinfd41-art/filem-transfers:transfers` widget to a bar.

The plugin requires Noctalia plugin API 24, introduced during the Noctalia 5.0
release cycle.
