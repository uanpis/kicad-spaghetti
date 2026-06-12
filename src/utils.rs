use glam::Vec2;

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
