// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2022-2024 GothicKit Contributors.
// SPDX-License-Identifier: MIT
use super::*;
fn common(r: &mut Reader<'_>, b: &mut Budget) -> Result<Common> {
    Ok(Common {
        name: line(r, b)?,
        layer: r.u32(CTX)?,
        next: line(r, b)?,
        blend_in: r.f32(CTX)?,
        blend_out: r.f32(CTX)?,
        flags: Flags(line(r, b)?),
    })
}
fn event(id: u16, r: &mut Reader<'_>, b: &mut Budget) -> Result<Event> {
    let frame = r.i32(CTX)?;
    Ok(match id {
        0xf5a1 | 0xf5a2 => {
            let s = Sound {
                frame,
                name: line(r, b)?,
                range: r.f32(CTX)?,
                empty_slot: Bool32(r.u32(CTX)?),
            };
            if id == 0xf5a1 {
                Event::Sound(s)
            } else {
                Event::GroundSound(s)
            }
        }
        0xf5a3 => {
            let name = line(r, b)?;
            let start = r.position();
            let mut arguments = vec![];
            while r.remaining() != 0 {
                let p = r.position();
                let v = line(r, b)?;
                push(&mut arguments, v, b, p)?;
            }
            let t = Tag {
                frame,
                name,
                arguments,
                attached: false,
            };
            validate_tag(&t, start)?;
            Event::Tag(t)
        }
        0xf5a4 => Event::Particle(Particle {
            frame,
            index: r.i32(CTX)?,
            name: line(r, b)?,
            position: line(r, b)?,
            attached: Bool32(r.u32(CTX)?),
            attachment_literal: None,
        }),
        0xf5a5 => Event::ParticleStop(ParticleStop {
            frame,
            index: r.i32(CTX)?,
        }),
        0xf5a9 => Event::Morph(Morph {
            frame,
            animation: line(r, b)?,
            node: line(r, b)?,
            compiled_values: Some([r.f32(CTX)?, r.f32(CTX)?]),
        }),
        0xf5aa => Event::CameraTremor(CameraTremor {
            frame,
            fields: [r.i32(CTX)?, r.i32(CTX)?, r.i32(CTX)?, r.i32(CTX)?],
        }),
        _ => return Err(at(ErrorKind::Unsupported, r.position() - 4)),
    })
}
#[derive(Clone, Copy)]
enum Owner {
    Animation(usize),
    Blend(usize),
    Combination(usize),
}
impl Owner {
    fn events(self, s: &mut ModelScript) -> &mut Vec<Event> {
        match self {
            Self::Animation(i) => &mut s.animations[i].events,
            Self::Blend(i) => &mut s.blends[i].events,
            Self::Combination(i) => &mut s.combinations[i].events,
        }
    }
}
pub(super) fn read(r: &mut Reader<'_>, b: &mut Budget) -> Result<ModelScript> {
    let mut s = ModelScript::default();
    let mut group: Option<(Owner, usize, usize)> = None;
    let mut owner = None;
    let mut ended = false;
    let mut model_open = false;
    let mut enum_open = false;
    while r.remaining() != 0 {
        let p = r.position();
        if ended {
            return Err(at(ErrorKind::Unsupported, p));
        }
        let (id, mut c) = chunk(r, b)?;
        if group.is_some() && !matches!(id, 0xf5a1..=0xf5a5 | 0xf5a9 | 0xf5aa | 0xf5af) {
            return Err(at(ErrorKind::InvalidData, p));
        }
        match id {
            0xf000 => {
                owner = None;
                if s.root.is_some() {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                s.root = Some(Root {
                    value: c.u32(CTX)?,
                    path: line(&mut c, b)?,
                });
            }
            0xf100 => {
                owner = None;
                if s.source.is_some() {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                s.source = Some(Source {
                    date: Date::read(&mut c, CTX)?,
                    path: line(&mut c, b)?,
                });
            }
            0xf300 => {
                owner = None;
                if s.skeleton.is_some() {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                s.skeleton = Some(Skeleton {
                    disable_mesh: Bool32(c.u32(CTX)?),
                    name: line(&mut c, b)?,
                });
            }
            0xf400 => {
                owner = None;
                let v = line(&mut c, b)?;
                push(&mut s.meshes, v, b, p)?;
            }
            0xf520 => {
                let v = Animation {
                    common: common(&mut c, b)?,
                    model: line(&mut c, b)?,
                    direction: Direction(line(&mut c, b)?),
                    first_frame: c.i32(CTX)?,
                    last_frame: c.i32(CTX)?,
                    fps: c.f32(CTX)?,
                    speed: c.f32(CTX)?,
                    collision_volume_scale: c.f32(CTX)?,
                    events: vec![],
                };
                push(&mut s.animations, v, b, p)?;
                owner = Some(Owner::Animation(s.animations.len() - 1));
            }
            0xf530 => {
                let v = Alias {
                    common: common(&mut c, b)?,
                    alias: line(&mut c, b)?,
                    direction: Direction(line(&mut c, b)?),
                };
                push(&mut s.aliases, v, b, p)?;
                owner = None;
            }
            0xf540 => {
                let v = Blend {
                    name: line(&mut c, b)?,
                    next: line(&mut c, b)?,
                    blend_in: c.f32(CTX)?,
                    blend_out: c.f32(CTX)?,
                    layer: None,
                    flags: None,
                    events: vec![],
                };
                push(&mut s.blends, v, b, p)?;
                owner = Some(Owner::Blend(s.blends.len() - 1));
            }
            0xf570 => {
                let v = Combination {
                    common: common(&mut c, b)?,
                    model: line(&mut c, b)?,
                    last_frame: c.i32(CTX)?,
                    events: vec![],
                };
                push(&mut s.combinations, v, b, p)?;
                owner = Some(Owner::Combination(s.combinations.len() - 1));
            }
            0xf580 => {
                let v = line(&mut c, b)?;
                push(&mut s.disabled_animations, v, b, p)?;
                owner = None;
            }
            0xf590 => {
                let v = ModelTag {
                    frame: Some(c.i32(CTX)?),
                    name: line(&mut c, b)?,
                    bone: line(&mut c, b)?,
                };
                push(&mut s.model_tags, v, b, p)?;
                owner = None;
            }
            0xf5a0 => {
                let index = owner.ok_or_else(|| at(ErrorKind::InvalidData, p))?;
                let count = c.u32(CTX)? as usize;
                let start = index.events(&mut s).len();
                group = Some((index, start, count));
                push(
                    &mut s.markers,
                    Marker {
                        id,
                        offset: p,
                        value: Some(count as u32),
                    },
                    b,
                    p,
                )?;
            }
            0xf5a1..=0xf5a5 | 0xf5a9 | 0xf5aa => {
                let index = owner.ok_or_else(|| at(ErrorKind::InvalidData, p))?;
                let v = event(id, &mut c, b)?;
                push(index.events(&mut s), v, b, p)?;
                if let Some((i, start, count)) = group
                    && i.events(&mut s).len() - start > count
                {
                    return Err(at(ErrorKind::InvalidData, p));
                }
            }
            0xf5af => {
                let (i, start, count) =
                    group.take().ok_or_else(|| at(ErrorKind::InvalidData, p))?;
                if i.events(&mut s).len() - start != count {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                push(
                    &mut s.markers,
                    Marker {
                        id,
                        offset: p,
                        value: None,
                    },
                    b,
                    p,
                )?;
            }
            0xf200 => {
                owner = None;
                if model_open {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                model_open = true;
                push(
                    &mut s.markers,
                    Marker {
                        id,
                        offset: p,
                        value: None,
                    },
                    b,
                    p,
                )?;
            }
            0xf500 => {
                owner = None;
                if enum_open {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                enum_open = true;
                push(
                    &mut s.markers,
                    Marker {
                        id,
                        offset: p,
                        value: None,
                    },
                    b,
                    p,
                )?;
            }
            0xf5ff => {
                owner = None;
                if !enum_open {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                enum_open = false;
                push(
                    &mut s.markers,
                    Marker {
                        id,
                        offset: p,
                        value: None,
                    },
                    b,
                    p,
                )?;
            }
            0xf2ff => {
                owner = None;
                if !model_open || enum_open {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                model_open = false;
                push(
                    &mut s.markers,
                    Marker {
                        id,
                        offset: p,
                        value: None,
                    },
                    b,
                    p,
                )?;
            }
            0xffff => {
                if model_open || enum_open {
                    return Err(at(ErrorKind::InvalidData, p));
                }
                ended = true;
                push(
                    &mut s.markers,
                    Marker {
                        id,
                        offset: p,
                        value: None,
                    },
                    b,
                    p,
                )?;
            }
            _ => return Err(at(ErrorKind::Unsupported, p)),
        }
        finish(&c)?;
    }
    if group.is_some() || model_open || enum_open || !ended {
        return Err(at(ErrorKind::InvalidData, r.position()));
    }
    Ok(s)
}
