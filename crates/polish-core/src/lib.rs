//! appshots-style image framing.

#[derive(Debug, Clone)]
pub struct PresentationStyle {
    pub seed: u64,
    pub palette_name: String,
    pub start: [u8; 3],
    pub end: [u8; 3],
    pub accent: [u8; 3],
    pub padding: u32,
    pub corner_radius: u32,
    pub shadow_blur: f32,
    pub shadow_offset_y: i32,
}

pub const PALETTES: [(&str, [u8; 3], [u8; 3], [u8; 3]); 5] = [
    ("dusk-berry", [34, 40, 78], [178, 48, 104], [118, 79, 178]),
    ("aurora-teal", [15, 77, 87], [62, 148, 126], [165, 212, 141]),
    ("graphite-rose", [38, 42, 49], [158, 64, 91], [222, 134, 113]),
    ("indigo-copper", [31, 45, 92], [190, 104, 62], [240, 167, 92]),
    ("forest-slate", [23, 65, 55], [73, 88, 103], [129, 160, 126]),
];

// Deterministic splitmix64 so the crate needs no rng dependency.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn range_u32(state: &mut u64, lo: u32, hi: u32) -> u32 {
    lo + (splitmix64(state) % u64::from(hi - lo + 1)) as u32
}

pub fn style_from_seed(seed: u64) -> PresentationStyle {
    let mut state = seed;
    let idx = (splitmix64(&mut state) % PALETTES.len() as u64) as usize;
    let (name, start, end, accent) = PALETTES[idx];
    PresentationStyle {
        seed,
        palette_name: name.to_string(),
        start,
        end,
        accent,
        padding: range_u32(&mut state, 56, 88),
        corner_radius: range_u32(&mut state, 14, 22),
        shadow_blur: range_u32(&mut state, 18, 30) as f32,
        shadow_offset_y: range_u32(&mut state, 14, 28) as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_from_seed_is_deterministic() {
        let a = style_from_seed(12345);
        let b = style_from_seed(12345);
        assert_eq!(a.palette_name, b.palette_name);
        assert_eq!(a.padding, b.padding);
        assert_eq!(a.start, b.start);
    }

    #[test]
    fn style_uses_a_known_palette() {
        let style = style_from_seed(1);
        assert!(PALETTES.iter().any(|p| p.0 == style.palette_name));
    }
}
