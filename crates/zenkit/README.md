# zenkit

Rust readers, writers and a Daedalus runtime for Gothic I and II data.
Tested on Windows x64 with Rust 1.99.0.

| Data or task | Modules | Support and limits |
| --- | --- | --- |
| Ordinary/Union VDF archives and virtual filesystems | `vfs` | Read, compose and write archives |
| Byte I/O and math records | `io`, `values` | Read and write |
| ASCII, BINARY and BIN_SAFE archives and object graphs | `archive`, `objects` | Read and write supported classes; unknown classes/versions are rejected |
| Materials, fonts and TEX textures | `material`, `font`, `texture` | Read and write; texture conversion and DDS export; no mip or palette generation |
| Meshes and models | `mesh` | Read and write; embedded material writers emit BINARY archives only |
| MAN animations and MMB morphs | `animation`, `morph` | Read only |
| MDS source and MSB model scripts | `model_script` | Read only; no compiler; some MSB chunks are unsupported |
| Cutscenes, VOBs, worlds and waypoint graphs | `cutscene`, `vob`, `world` | Read and write supported schemas |
| Save metadata/state and directories | `savegame` | Read, edit and write; directory replacement requires exclusive access |
| Daedalus DAT and runtime | `daedalus` | Read and inspect DAT; execute with host bindings; no DAT writer; `MOVSS` is unsupported |
| Diagnostic subscribers | `logging` | Application-selected callbacks |

Module documentation lists supported format versions, conversion rules and
unsupported records. Public tests use synthetic fixtures; compatibility with
arbitrary game installations or mods is not guaranteed.

Add a path dependency:

```toml
[dependencies]
zenkit = { path = "/path/to/ZenKit-Rust/crates/zenkit" }
```

Compression is included by default. The crate requires Rust 1.99 or newer;
this workspace pins 1.99.0.

```rust,no_run
use zenkit::vfs::{ReadOptions, Vdf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("MYMOD.VDF")?;
    let archive = Vdf::from_owned(bytes, ReadOptions::default())?;
    if let Some(entry) = archive.resolve(b"DATA/EXAMPLE.ZEN") {
        if let Some(payload) = entry.payload() {
            println!("{} bytes", payload.len());
        }
    }
    Ok(())
}
```

Names and paths stay raw bytes. Callers choose text decoding and a reversible
codec for host filenames. Each module has resource limits; these apply to
individual operations, not all process allocations. Errors retain their kind,
available offsets and paths, and underlying I/O or host causes.

## Editing object graphs

The `object_editing` example opens an existing archive, changes its NPC guilds,
and writes the edited document:

```text
cargo run -p zenkit --example object_editing --locked -- g2 WORLD.ZEN EDITED.ZEN 42
```

Graphs own their values and preserve aliases and cycles. IDs belong to one
graph; after reopening, obtain IDs from its roots and links.

| Expected schema | Read | Edit |
| --- | --- | --- |
| Any VOB subtype | `vob(id) -> Result<VobRef>` | `vob_mut(id) -> Result<VobMut>` |
| NPC | `npc(id) -> Result<&NpcData>` | `npc_mut(id) -> Result<&mut NpcData>` |
| Item | `item(id) -> Result<&Item>` | `item_mut(id) -> Result<&mut Item>` |
| Container | `container(id) -> Result<&Container>` | `container_mut(id) -> Result<&mut Container>` |
| Material | `material(id) -> Result<&Material>` | `material_mut(id) -> Result<&mut Material>` |
| World | `world(id) -> Result<&World>` | `world_mut(id) -> Result<&mut World>` |

Accessors borrow stored data without allocation or cloning. They return
`InvalidHandle` for another graph's ID and `WrongObjectType` for another schema.
The VOB accessor accepts every VOB subtype; other accessors require an exact match.

Owned VOBs expose `base` and `kind` fields. Borrowed views use
`base()`/`base_mut()` and `kind()`/`kind_mut()`:

```rust
use zenkit::{GameVersion, objects::{Graph, Object}, vob::*};

fn main() -> zenkit::Result<()> {
    let game = GameVersion::Gothic2;
    let mut graph = Graph::new()?;
    let mut value = Vob::new(game);
    value.base.show_visual = true;
    value.kind = VobKind::Npc(NpcData::new(game));
    let id = graph.insert(Object::Vob(value))?;

    let npc = graph.npc_mut(id)?;
    npc.guild = 42;
    npc.level = 12;
    assert_eq!(graph.npc(id)?.guild, 42);
    graph.vob_mut(id)?.base_mut().show_visual = false;
    Ok(())
}
```

Read other objects' IDs and scalars before mutably borrowing a payload.
For example, an NPC inventory shortcut depends on its Item's flags:

```rust
use zenkit::{objects::{Graph, ObjectId}, vob::*};
fn set_shortcut(graph: &mut Graph, npc: ObjectId, slot: usize, key: i32)
    -> zenkit::Result<()>
{
    // The caller supplies an existing inventory slot.
    let item = graph.npc(npc)?.items[slot].item.target;
    let needs_key = match item {
        Some(id) => graph.item(id)?.state.as_ref()
            .is_some_and(|state| state.flags & 0x200 != 0),
        None => false,
    };
    graph.npc_mut(npc)?.items[slot].short_key = needs_key.then_some(key);
    Ok(())
}
```

Several inventory entries can share an Item and have different shortcuts.
Edits are not batch transactions: a later error leaves earlier edits applied.

Use `into_parts()` when retaining both common and subtype borrows from one VOB:

```rust
use zenkit::{objects::{Graph, ObjectId}, vob::*};
fn edit_npc(graph: &mut Graph, id: ObjectId) -> zenkit::Result<()> {
    let (base, kind) = graph.vob_mut(id)?.into_parts();
    if let VobKindMut::Npc(npc) = kind {
        base.show_visual = true;
        npc.guild_true = npc.guild;
    }
    Ok(())
}
```

`kind_mut()` returns a borrowed enum by value. Match its variant to edit the
payload. For mixed object traversal, match `ObjectRef`/`ObjectMut` values from
`get`, `get_mut` or `objects`.

Use `Graph::replace` to replace an object or `replace_vob_kind` to replace its
subtype. Both retain the ID, return the old payload and clear incompatible
class/version aliases. Allocation failure leaves the graph unchanged.
`ObjectRef::to_owned` clones one payload; its links still belong to the source graph.

Validate drafts before writing. `ObjectArchive::validate` and `to_bytes` check all
nodes, including orphans, game/save state and edge kinds. Direct field edits can
invalidate retained version aliases; use `Graph::clear_record_version` to clear
an incompatible alias. Module rustdoc covers format-specific conversion and
unknown-data policies.

## Examples

Each example demonstrates one application task using caller-supplied files.
Run it with `cargo run -p zenkit --example NAME --locked -- ARGUMENTS`.

| Example | Arguments | Task |
| --- | --- | --- |
| `open_vdf` | `INPUT.vdf` | List archive files and decoded sizes |
| `compose_vfs` | `INPUT.vdf NOTE.txt OUTPUT.vdf` | Insert a file and write a compressed archive |
| `archive` | `INPUT.zen` | Inspect root object headers without class schemas |
| `world` | `g1\|g2 INPUT.zen` | List world VOB names, classes, positions and parents |
| `object_editing` | `g1\|g2 INPUT.zen OUTPUT.zen GUILD` | Edit NPC guilds and save the document |
| `texture` | `INPUT.tex OUTPUT.dds` | Export a texture to DDS |
| `savegame` | `g1\|g2 INPUT_DIRECTORY NEW_DIRECTORY` | Edit a save's day counter and publish a copy |
| `daedalus` | `INPUT.dat` | Inspect script symbols without executing code |
| `daedalus_vm` | `INPUT.dat FUNCTION` | Execute a parameterless function with no host dependencies |

For example, inspect a standalone world with:

```text
cargo run -p zenkit --example world --locked -- g2 WORLD.ZEN
```

The world example walks each VOB hierarchy in file order. Parent indices belong
to the displayed section; `None` denotes a root. Shared VOBs appear at every
occurrence, and null entries remain visible so parent indices stay meaningful.
Names use lossy UTF-8 for display only. Its tests construct small worlds in
memory for both games and all three archive encodings; no game files are used.

Choose output paths separate from the input files. The savegame destination
must be a new directory. The VM example executes the supplied script; games
that require external functions or instance members need application host bindings.

The public API covers file operations, editable domain data, host integration,
errors and resource limits. Byte readers and writers support public serializers;
generic archive readers and writers support schema-directed tools. Compression
framing, embedded archive address spaces and parser work accounting are internal.

Synthetic round trips, subtype coverage and game/encoding matrices live in the
tests, including `tests/workflows/`. Internal parser tests live beside the source.
The packaged fixtures contain generated data:

```text
cargo test --all-targets --locked
cargo test --doc --locked
cargo doc --no-deps --locked
```
