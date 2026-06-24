use crate::gui::ColorTheme;
use glam::Vec2;

pub const fn hex_color(hex: u32) -> [f32; 4] {
    [
        ((hex >> 24) & 0xFF) as f32 / 256.0,
        ((hex >> 16) & 0xFF) as f32 / 256.0,
        ((hex >> 8) & 0xFF) as f32 / 256.0,
        (hex & 0xFF) as f32 / 256.0,
    ]
}

fn f32_to_ordered_u32(f: f32) -> u32 {
    let bits = f.to_bits();
    if bits >> 31 == 0 {
        bits | 0x8000_0000
    } else {
        !bits
    }
}

fn spread(x: u32) -> u64 {
    let mut x = x as u64;
    x = (x | (x << 16)) & 0x0000_FFFF_0000_FFFF;
    x = (x | (x << 8)) & 0x00FF_00FF_00FF_00FF;
    x = (x | (x << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
    x = (x | (x << 2)) & 0x3333_3333_3333_3333;
    x = (x | (x << 1)) & 0x5555_5555_5555_5555;
    x
}

pub fn morton(pos: Vec2) -> u64 {
    let xi = f32_to_ordered_u32(pos.x);
    let yi = f32_to_ordered_u32(pos.y);
    spread(xi) | (spread(yi) << 1)
}

pub fn powu(mut base: f32, exp: u32) -> f32 {
    match exp {
        0 => 1.0,
        1 => base,
        2 => base * base,
        3 => base * base * base,
        4 => base * base * base * base,
        5 => base * base * base * base * base,
        _ => {
            let mut e = exp;
            let mut result = 1.0;
            while e != 0 {
                if e & 1 != 0 {
                    result *= base;
                }
                base *= base;
                e >>= 1;
            }
            result
        }
    }
}

pub trait ToMm<T> {
    fn to_mm(self) -> T;
}

impl ToMm<Vec2> for kicad_ipc_rs::model::board::Vector2Nm {
    fn to_mm(self) -> Vec2 {
        Vec2::new(self.x_nm.to_mm(), self.y_nm.to_mm())
    }
}

impl ToMm<Vec2> for Option<kicad_ipc_rs::model::board::Vector2Nm> {
    fn to_mm(self) -> Vec2 {
        if let Some(v) = self {
            v.to_mm()
        } else {
            Vec2::ZERO
        }
    }
}

impl ToMm<f32> for i64 {
    fn to_mm(self) -> f32 {
        1e-6f32 * self as f32
    }
}

impl ToMm<f32> for Option<i64> {
    fn to_mm(self) -> f32 {
        if let Some(x) = self { x.to_mm() } else { 0.0 }
    }
}

pub trait Resettable<T> {
    fn get(&self) -> T;
    fn get_mut(&mut self) -> &mut T;
    fn set(&mut self, value: T);
    fn reset(&mut self);
}

macro_rules! impl_resettable {
    ($n:ident, $t:ty) => {
        #[derive(Clone, Copy, PartialEq, Debug)]
        pub struct $n {
            pub value: $t,
            pub default: $t,
        }

        impl $n {
            fn new(default: $t) -> Self {
                Self {
                    value: default,
                    default,
                }
            }
        }

        impl Resettable<$t> for $n {
            fn get(&self) -> $t {
                self.value
            }
            fn get_mut(&mut self) -> &mut $t {
                &mut self.value
            }
            fn set(&mut self, value: $t) {
                self.value = value;
            }
            fn reset(&mut self) {
                self.value = self.default;
            }
        }

        impl From<$n> for $t {
            fn from(m: $n) -> Self {
                m.value
            }
        }

        impl From<$t> for $n {
            fn from(m: $t) -> Self {
                $n::new(m)
            }
        }
    };
}

pub(crate) use impl_resettable;

impl_resettable!(F32Resettable, f32);
impl_resettable!(U32Resettable, u32);
impl_resettable!(UsizeResettable, usize);
impl_resettable!(BoolResettable, bool);
impl_resettable!(ColorThemeResettable, ColorTheme);
