# Bundled test fonts

The proportional layout snapshot suite (`layout_snapshots.rs`) renders through
`rusttype` using deterministic, machine-independent font metrics. The NotoSans
faces are vendored here so the tests need no system fonts or network.

NotoSans is licensed under the **SIL Open Font License 1.1** (see `OFL.txt`),
which permits redistribution alongside software.

| File | Family | Upstream |
| --- | --- | --- |
| `NotoSans-Medium.ttf` | Noto Sans | google/fonts `ofl/notosans` |
| `NotoSans-Bold.ttf` | Noto Sans | google/fonts `ofl/notosans` |
| `NotoSans-MediumItalic.ttf` | Noto Sans | google/fonts `ofl/notosans` |
| `NotoSans-BoldItalic.ttf` | Noto Sans | google/fonts `ofl/notosans` |

The faces use the Medium weight in the "regular" slot to match Piki's original
snapshot backend, so its proportional snapshots port over byte-for-byte.

The **monospace suite** (`layout_snapshots_mono.rs`) bundles no font: a fixed
cell grid is fully defined by a cell width and row height, so its metrics are
synthesized from constants (see `FontMode::Monospace` in `common/mod.rs`) and
the SVG references the generic `monospace` family for browser preview.
