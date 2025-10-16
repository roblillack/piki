use crate::draw_context::{FontStyle, FontType};

#[derive(Clone, Copy, Debug)]
pub struct FontSettings {
    pub font_type: FontType,
    pub font_style: FontStyle,
    pub font_size: u8,
    pub font_color: u32,
    pub background_color: Option<u32>,
}

pub struct Theme {
    pub background_color: u32,
    pub selection_color: u32,
    pub cursor_color: u32,

    pub quote_bar_color: u32,
    pub quote_bar_width: u32,

    pub link_color: u32,
    pub link_hover_background: u32,
    pub link_hover_color: u32,

    pub padding_vertical: i32,
    pub padding_horizontal: i32,

    pub line_height: i32,

    pub header_level_1: FontSettings,
    pub header_level_2: FontSettings,
    pub header_level_3: FontSettings,
    pub plain_text: FontSettings,
    pub quote_text: FontSettings,
    pub code_text: FontSettings,
}

impl Theme {
    pub fn default() -> Self {
        Self {
            background_color: 0xFFFFF5FF,      // Off-white background
            selection_color: 0xB4D5FEFF,       // Light blue selection color
            cursor_color: 0x000000FF,          // Black cursor
            quote_bar_color: 0xCCCCCCFF,       // Light gray quote bar
            quote_bar_width: 4,                // Width of the quote bar
            link_color: 0x0000EEFF,            // Standard blue link color
            link_hover_background: 0xDDDDDDFF, // Light gray hover background
            link_hover_color: 0x0000AAFF,      // Darker blue link color

            padding_vertical: 10,
            padding_horizontal: 25,
            line_height: 17,
            header_level_1: FontSettings {
                font_type: FontType::Heading,
                font_style: FontStyle::Bold,
                font_size: 24,
                font_color: 0x000000FF,
                background_color: None,
            },
            header_level_2: FontSettings {
                font_type: FontType::Heading,
                font_style: FontStyle::Bold,
                font_size: 20,
                font_color: 0x000000FF,
                background_color: None,
            },
            header_level_3: FontSettings {
                font_type: FontType::Heading,
                font_style: FontStyle::Bold,
                font_size: 18,
                font_color: 0x000000FF,
                background_color: None,
            },
            plain_text: FontSettings {
                font_type: FontType::Content,
                font_style: FontStyle::Regular,
                font_size: 14,
                font_color: 0x000000FF,
                background_color: None,
            },
            quote_text: FontSettings {
                font_type: FontType::Content,
                font_style: FontStyle::Italic,
                font_size: 14,
                font_color: 0x555555FF,
                background_color: None,
            },
            code_text: FontSettings {
                font_type: FontType::Code,
                font_style: FontStyle::Regular,
                font_size: 14,
                font_color: 0x0064C8FF,
                background_color: None,
            },
        }
    }
}
