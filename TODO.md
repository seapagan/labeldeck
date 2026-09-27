# TODO Items

- Make canonical deck writes crash-safe: write to a temporary file in the
  destination directory, flush/sync as appropriate, then atomically replace the
  target so write failures cannot leave a partial/truncated `labels.json`
  (including `--force` updates).
