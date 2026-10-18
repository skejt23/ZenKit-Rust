use crate::{
    Options,
    assets::{Assets, key, with_extension},
};
use anyhow::{Context, Result, bail};
use glam::{Mat3, Mat4, Vec3};
use std::collections::{BTreeMap, HashMap};
use zenkit::{
    GameVersion,
    material::{AlphaFunction, Material, MaterialVersion},
    mesh::{Mesh, MultiResolutionMesh},
    objects::ObjectArchive,
    world::SectionData,
};

type Vertex = [f32; 12]; // position, normal, UV, linear RGBA

pub struct Surface {
    pub texture: Vec<u8>,
    pub blend: bool,
}
pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub instances: Vec<[f32; 16]>,
    pub material: usize,
}
pub struct Scene {
    pub primitives: Vec<Primitive>,
    pub materials: Vec<Surface>,
    pub spawn: Vec3,
    pub target: Vec3,
    pub vobs: usize,
    material_ids: HashMap<(Vec<u8>, bool), usize>,
}

// Gothic uses centimetres, Y up, +Z forward. Reflect Z for our right-handed view.
fn point(p: zenkit::values::Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, -p.z) * 0.01
}
fn normal(p: zenkit::values::Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, -p.z).normalize_or_zero()
}
fn vob_transform(base: &zenkit::vob::VobBase) -> Mat4 {
    let r = &base.rotation;
    let reflect = Mat3::from_diagonal(Vec3::new(1.0, 1.0, -1.0));
    let rotation = Mat3::from_cols(
        Vec3::from_array(r[0].into()),
        Vec3::from_array(r[1].into()),
        Vec3::from_array(r[2].into()),
    );
    Mat4::from_translation(point(base.position)) * Mat4::from_mat3(reflect * rotation * reflect)
}
fn model_transform(m: zenkit::values::Mat4) -> Mat4 {
    let original = Mat4::from_cols_array(&std::array::from_fn(|i| m[i / 4][i % 4]));
    let convert = Mat4::from_scale(Vec3::new(0.01, 0.01, -0.01));
    convert * original * convert.inverse()
}
fn model_poses(hierarchy: &zenkit::mesh::ModelHierarchy) -> Result<Vec<Mat4>> {
    // The file permits parents after children. Resolve each chain iteratively.
    hierarchy.validate(Default::default())?;
    let mut poses = vec![None; hierarchy.nodes.len()];
    for start in 0..poses.len() {
        let mut chain = vec![];
        let mut current = Some(start);
        while let Some(i) = current {
            if poses[i].is_some() {
                break;
            }
            chain.push(i);
            current = usize::try_from(hierarchy.nodes[i].parent).ok();
        }
        while let Some(i) = chain.pop() {
            let node = &hierarchy.nodes[i];
            let local = model_transform(node.transform);
            let parent = usize::try_from(node.parent)
                .ok()
                .map_or(Mat4::IDENTITY, |p| poses[p].unwrap());
            poses[i] = Some(parent * local);
        }
    }
    Ok(poses.into_iter().map(Option::unwrap).collect())
}
fn linear(c: u8) -> f32 {
    let x = f32::from(c) / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn color(material: &Material, light: Option<u32>) -> [f32; 4] {
    let c = material.color;
    let rgb = if material.texture.is_empty() {
        [c.r, c.g, c.b]
    } else {
        [255; 3]
    };
    let baked = light.map(|v| {
        [
            ((v >> 16) & 255) as u8,
            ((v >> 8) & 255) as u8,
            (v & 255) as u8,
        ]
    });
    [
        linear(rgb[0]) * baked.map_or(1.0, |c| linear(c[0]).max(0.2)),
        linear(rgb[1]) * baked.map_or(1.0, |c| linear(c[1]).max(0.2)),
        linear(rgb[2]) * baked.map_or(1.0, |c| linear(c[2]).max(0.2)),
        if c.a == 0 {
            1.0
        } else {
            f32::from(c.a) / 255.0
        },
    ]
}
fn vertex(p: Vec3, n: Vec3, uv: [f32; 2], c: [f32; 4]) -> Result<Vertex> {
    let v = [
        p.x, p.y, p.z, n.x, n.y, n.z, uv[0], uv[1], c[0], c[1], c[2], c[3],
    ];
    if v.iter().any(|v| !v.is_finite()) {
        bail!("nonfinite vertex in render geometry");
    }
    Ok(v)
}

impl Scene {
    pub fn load(options: &Options, assets: &Assets) -> Result<Self> {
        eprintln!("Loading {}", options.world);
        let bytes = assets.world(&options.world)?;
        let document =
            ObjectArchive::from_bytes(&bytes, options.game_version(), Default::default())
                .context("reading world; check --game g1/g2")?;
        let [root] = document.graph.roots() else {
            bail!("expected one world root");
        };
        let world = document
            .graph
            .world(root.target.context("null world root")?)?;
        let mut scene = Self {
            primitives: vec![],
            materials: vec![],
            spawn: Vec3::ZERO,
            target: Vec3::ZERO,
            vobs: 0,
            material_ids: HashMap::new(),
        };
        let mut start = None;
        for section in &world.sections {
            if let SectionData::MeshAndBsp(geometry) = &section.data {
                let triangles = geometry.leaf_triangles(1024, 10_000_000, 10_000_000)?;
                let ids = scene.add_msh(&geometry.mesh, &triangles)?;
                for id in ids {
                    scene.primitives[id]
                        .instances
                        .push(Mat4::IDENTITY.to_cols_array());
                }
            }
        }
        let mut cache: HashMap<Vec<u8>, Option<Vec<usize>>> = HashMap::new();
        let mut skipped = BTreeMap::<String, usize>::new();
        for section in &world.sections {
            let SectionData::VobTree(tree) = &section.data else {
                continue;
            };
            for entry in &tree.entries {
                let Some(id) = entry.object.target else {
                    continue;
                };
                let vob = document.graph.vob(id)?;
                let base = vob.base();
                if base.name.as_deref().is_some_and(|n| {
                    n.eq_ignore_ascii_case(b"START_GOTHIC2") || n.eq_ignore_ascii_case(b"START")
                }) {
                    let position = point(base.position) + Vec3::Y * 1.7;
                    let direction = vob_transform(base)
                        .transform_vector3(-Vec3::Z)
                        .normalize_or_zero();
                    start.get_or_insert((position, position + direction));
                }
                if options.no_vobs || !base.show_visual {
                    continue;
                }
                let Some(name) = base.visual_name.as_deref().filter(|n| !n.is_empty()) else {
                    continue;
                };
                let name_key = key(name);
                if !cache.contains_key(&name_key) {
                    let result = scene.add_visual(name, options.game_version(), assets);
                    let ids = match result {
                        Ok(Some(ids)) if !ids.is_empty() => Some(ids),
                        Ok(_) => None,
                        Err(e) => {
                            eprintln!("Skipping {}: {e:#}", String::from_utf8_lossy(name));
                            None
                        }
                    };
                    cache.insert(name_key.clone(), ids);
                }
                if let Some(ids) = &cache[&name_key] {
                    // VOB transforms are already absolute; never accumulate the occurrence parent.
                    let transform = vob_transform(base).to_cols_array();
                    if transform.iter().any(|v| !v.is_finite()) {
                        bail!("nonfinite VOB transform");
                    }
                    for &id in ids {
                        scene.primitives[id].instances.push(transform);
                    }
                    scene.vobs += 1;
                } else {
                    *skipped
                        .entry(String::from_utf8_lossy(name).into_owned())
                        .or_default() += 1;
                }
            }
        }
        scene
            .primitives
            .retain(|p| !p.vertices.is_empty() && !p.instances.is_empty());
        if scene.primitives.is_empty() {
            bail!(
                "world has no renderable mesh; saved worlds often omit terrain, use an original .ZEN and its assets"
            );
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for primitive in &scene.primitives {
            for transform in &primitive.instances {
                let transform = Mat4::from_cols_array(transform);
                for v in &primitive.vertices {
                    let p = transform.transform_point3(Vec3::new(v[0], v[1], v[2]));
                    min = min.min(p);
                    max = max.max(p);
                }
            }
        }
        let center = (min + max) * 0.5;
        let radius = (max - min).length().max(10.0) * 0.5;
        (scene.spawn, scene.target) =
            start.unwrap_or((center + Vec3::new(0.0, radius * 0.5, radius * 0.9), center));
        let triangles: usize = scene
            .primitives
            .iter()
            .map(|p| p.vertices.len() / 3 * p.instances.len())
            .sum();
        eprintln!(
            "Scene: {triangles} triangles, {} static VOBs, {} mesh batches, {} materials",
            scene.vobs,
            scene.primitives.len(),
            scene.materials.len()
        );
        if !skipped.is_empty() {
            let count: usize = skipped.values().sum();
            eprintln!(
                "Skipped {count} unavailable/unsupported visuals ({} unique); first 20:",
                skipped.len()
            );
            for (name, count) in skipped.iter().take(20) {
                eprintln!("  {count:5}  {name}");
            }
        }
        Ok(scene)
    }

    fn material(&mut self, m: &Material) -> usize {
        let blend = matches!(&m.version, MaterialVersion::Gothic2(v) if v.alpha_func == AlphaFunction::BLEND);
        let k = (key(&m.texture), blend);
        *self.material_ids.entry(k).or_insert_with(|| {
            let id = self.materials.len();
            self.materials.push(Surface {
                texture: m.texture.clone(),
                blend,
            });
            id
        })
    }
    fn push(&mut self, vertices: Vec<Vertex>, material: usize) -> usize {
        let id = self.primitives.len();
        self.primitives.push(Primitive {
            vertices,
            instances: vec![],
            material,
        });
        id
    }
    fn add_msh(&mut self, mesh: &Mesh, triangles: &[zenkit::mesh::Triangle]) -> Result<Vec<usize>> {
        let mut groups = BTreeMap::<u16, Vec<Vertex>>::new();
        for triangle in triangles {
            let m = &mesh.materials.records[usize::from(triangle.material)].material;
            let vertices = groups.entry(triangle.material).or_default();
            for corner in triangle.corners {
                let f = mesh.features[corner.feature as usize];
                vertices.push(vertex(
                    point(mesh.vertices[corner.vertex as usize]),
                    normal(f.normal),
                    f.texture.into(),
                    color(m, Some(f.light)),
                )?);
            }
        }
        Ok(groups
            .into_iter()
            .map(|(m, vertices)| {
                let material = self.material(&mesh.materials.records[usize::from(m)].material);
                self.push(vertices, material)
            })
            .collect())
    }
    fn add_mrm(&mut self, mesh: &MultiResolutionMesh, transform: Mat4) -> Result<Vec<usize>> {
        let mut ids = vec![];
        for sub in &mesh.submeshes {
            let m = &sub.material.material;
            let material = self.material(m);
            let mut vertices = Vec::with_capacity(sub.triangles.len() * 3);
            for triangle in &sub.triangles {
                for &w in triangle {
                    let wedge = sub.wedges[usize::from(w)];
                    vertices.push(vertex(
                        transform.transform_point3(point(mesh.positions[usize::from(wedge.index)])),
                        transform
                            .transform_vector3(normal(wedge.normal))
                            .normalize_or_zero(),
                        wedge.texture.into(),
                        color(m, None),
                    )?);
                }
            }
            ids.push(self.push(vertices, material));
        }
        Ok(ids)
    }
    fn add_model(&mut self, model: &zenkit::mesh::Model) -> Result<Vec<usize>> {
        let poses = model_poses(&model.hierarchy)?;
        let mut ids = vec![];
        for attachment in &model.mesh.attachments {
            let node = model
                .hierarchy
                .nodes
                .iter()
                .position(|n| n.name.eq_ignore_ascii_case(&attachment.name))
                .context("model attachment has no hierarchy node")?;
            ids.extend(self.add_mrm(&attachment.mesh, poses[node])?);
        }
        Ok(ids)
    }
    fn add_visual(
        &mut self,
        name: &[u8],
        game: GameVersion,
        assets: &Assets,
    ) -> Result<Option<Vec<usize>>> {
        let ext = key(name)
            .rsplit(|c| *c == b'.')
            .next()
            .unwrap_or_default()
            .to_vec();
        let candidates: &[&[u8]] = match ext.as_slice() {
            b"3DS" | b"MRM" => &[b"MRM", b"MSH"],
            b"MSH" => &[b"MSH"],
            b"ASC" | b"MDS" | b"MDL" | b"MDM" => &[b"MDL", b"MDM"],
            b"MMS" | b"MMB" => &[b"MMB"],
            _ => return Ok(None),
        };
        for &ext in candidates {
            let file = with_extension(name, ext);
            let Some(bytes) = assets.read(&file)? else {
                continue;
            };
            let limits = zenkit::mesh::Limits::default();
            let ids = match ext {
                b"MRM" => self.add_mrm(
                    &MultiResolutionMesh::from_bytes(&bytes, game, limits)?,
                    Mat4::IDENTITY,
                )?,
                b"MSH" => {
                    let mesh = Mesh::from_bytes(&bytes, game, false, limits)?;
                    let triangles = mesh.triangulate_leaves(
                        &(0..mesh.polygons.len() as u32).collect::<Vec<_>>(),
                        10_000_000,
                    )?;
                    self.add_msh(&mesh, &triangles)?
                }
                b"MMB" => self.add_mrm(
                    &zenkit::morph::MorphMesh::from_bytes(&bytes, game, Default::default())?.mesh,
                    Mat4::IDENTITY,
                )?,
                b"MDL" => {
                    self.add_model(&zenkit::mesh::Model::from_bytes(&bytes, game, limits)?)?
                }
                b"MDM" => {
                    let hierarchy = with_extension(name, b"MDH");
                    let Some(hierarchy) = assets.read(&hierarchy)? else {
                        return Ok(None);
                    };
                    let model = zenkit::mesh::Model {
                        hierarchy: zenkit::mesh::ModelHierarchy::from_bytes(&hierarchy, limits)?,
                        mesh: zenkit::mesh::ModelMesh::from_bytes(&bytes, game, limits)?,
                    };
                    self.add_model(&model)?
                }
                _ => unreachable!(),
            };
            return Ok(Some(ids));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vob_rotation_and_translation_survive_coordinate_conversion() {
        let mut base = zenkit::vob::Vob::new(GameVersion::Gothic2).base;
        base.position = [100.0, 200.0, 300.0].into();
        base.rotation = zenkit::values::Mat3::from_rows(
            [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]].map(Into::into),
        );
        let p = vob_transform(&base).transform_point3(point([100.0, 0.0, 0.0].into()));
        assert!(p.abs_diff_eq(Vec3::new(1.0, 2.0, -2.0), 0.0001));
    }
    #[test]
    fn model_hierarchy_handles_parents_after_children() {
        use zenkit::{
            mesh::{HierarchyNode, ModelHierarchy},
            values,
        };
        let mut translation = values::Mat4::IDENTITY;
        translation[3] = [100.0, 200.0, 300.0, 1.0].into();
        let hierarchy = ModelHierarchy {
            nodes: vec![
                HierarchyNode {
                    name: b"child".to_vec(),
                    parent: 1,
                    transform: translation,
                },
                HierarchyNode {
                    name: b"parent".to_vec(),
                    parent: -1,
                    transform: translation,
                },
            ],
            ..Default::default()
        };
        let poses = model_poses(&hierarchy).unwrap();
        assert!(
            poses[0]
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(2.0, 4.0, -6.0), 0.0001)
        );
    }
}
