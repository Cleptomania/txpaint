#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Color(pub [u8; 4]);

impl Color {
    pub const BLACK: Color = Color([0, 0, 0, 255]);
    pub const WHITE: Color = Color([255, 255, 255, 255]);

    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color([r, g, b, 255])
    }

    pub fn to_egui(self) -> egui::Color32 {
        egui::Color32::from_rgba_unmultiplied(self.0[0], self.0[1], self.0[2], self.0[3])
    }

    pub fn to_linear_f32(self) -> [f32; 4] {
        fn s2l(c: u8) -> f32 {
            let x = c as f32 / 255.0;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        }
        [
            s2l(self.0[0]),
            s2l(self.0[1]),
            s2l(self.0[2]),
            self.0[3] as f32 / 255.0,
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub name: String,
    pub colors: Vec<Color>,
}

impl Palette {
    pub fn default_dos_variant(name: impl Into<String>) -> Self {
        let mut p = Self::default_dos();
        p.name = name.into();
        p
    }

    pub fn default_dos() -> Self {
        // Classic DOS 16-color palette.
        let colors = vec![
            Color::rgb(0, 0, 0),
            Color::rgb(0, 0, 170),
            Color::rgb(0, 170, 0),
            Color::rgb(0, 170, 170),
            Color::rgb(170, 0, 0),
            Color::rgb(170, 0, 170),
            Color::rgb(170, 85, 0),
            Color::rgb(170, 170, 170),
            Color::rgb(85, 85, 85),
            Color::rgb(85, 85, 255),
            Color::rgb(85, 255, 85),
            Color::rgb(85, 255, 255),
            Color::rgb(255, 85, 85),
            Color::rgb(255, 85, 255),
            Color::rgb(255, 255, 85),
            Color::rgb(255, 255, 255),
        ];
        Self {
            name: "DOS 16".into(),
            colors,
        }
    }
}
