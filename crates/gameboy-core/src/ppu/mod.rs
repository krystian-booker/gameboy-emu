pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;
pub const FRAMEBUFFER_PIXELS: usize = SCREEN_WIDTH * SCREEN_HEIGHT;

use crate::memory::MemoryRegion;

const DOTS_PER_LINE: u32 = 456;
const VISIBLE_LINES: u8 = 144;
const LINES_PER_FRAME: u8 = 154;
const MODE_2_DOTS: u32 = 80;
const MODE_3_BASE_DOTS: u32 = 172;
const VRAM_SIZE: usize = 0x2000;
const OAM_SIZE: usize = 0xA0;
const OAM_ENTRY_BYTES: usize = 4;
const SPRITE_COUNT: usize = 40;
const MAX_SPRITES_PER_LINE: usize = 10;
const TILE_BYTES: usize = 16;
const TILE_MAP_SIZE: usize = 32;
const CGB_PALETTE_RAM_SIZE: usize = 64;
const DMG_COLORS: [u32; 4] = [0xFFFF_FFFF, 0xFFAA_AAAA, 0xFF55_5555, 0xFF00_0000];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PpuMode {
    #[default]
    HBlank = 0,
    VBlank = 1,
    OamScan = 2,
    Drawing = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Ppu {
    lcdc: u8,
    stat: u8,
    scy: u8,
    scx: u8,
    ly: u8,
    lyc: u8,
    wy: u8,
    wx: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
    mode: PpuMode,
    line_dots: u32,
    window_line: u8,
    frame_ready: bool,
    cgb: bool,
    vbk: u8,
    bcps: u8,
    ocps: u8,
    #[serde(with = "crate::memory::byte_array")]
    bg_palette_ram: [u8; CGB_PALETTE_RAM_SIZE],
    #[serde(with = "crate::memory::byte_array")]
    obj_palette_ram: [u8; CGB_PALETTE_RAM_SIZE],
    vram: [MemoryRegion<VRAM_SIZE>; 2],
    oam: MemoryRegion<OAM_SIZE>,
    framebuffer: Vec<u32>,
    bg_color_ids: Vec<u8>,
    bg_attr_priority: Vec<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PpuInterrupts {
    pub vblank: bool,
    pub stat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Sprite {
    oam_index: usize,
    x: i16,
    y: i16,
    tile: u8,
    attributes: u8,
    line: usize,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            lcdc: 0x91,
            stat: 0x85,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            wy: 0,
            wx: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            mode: PpuMode::OamScan,
            line_dots: 0,
            window_line: 0,
            frame_ready: false,
            cgb: false,
            vbk: 0,
            bcps: 0,
            ocps: 0,
            bg_palette_ram: [0; CGB_PALETTE_RAM_SIZE],
            obj_palette_ram: [0; CGB_PALETTE_RAM_SIZE],
            vram: [MemoryRegion::default(), MemoryRegion::default()],
            oam: MemoryRegion::default(),
            framebuffer: vec![0; FRAMEBUFFER_PIXELS],
            bg_color_ids: vec![0; FRAMEBUFFER_PIXELS],
            bg_attr_priority: vec![false; FRAMEBUFFER_PIXELS],
        }
    }
}

impl Ppu {
    pub fn advance_cycles(&mut self, cycles: u32) -> PpuInterrupts {
        let mut interrupts = PpuInterrupts::default();
        if !self.lcd_enabled() {
            return interrupts;
        }

        self.line_dots += cycles;
        while self.line_dots >= DOTS_PER_LINE {
            if self.ly < VISIBLE_LINES {
                self.render_scanline();
            }

            self.line_dots -= DOTS_PER_LINE;
            self.ly = (self.ly + 1) % LINES_PER_FRAME;

            if self.ly == VISIBLE_LINES {
                self.set_mode(PpuMode::VBlank);
                self.frame_ready = true;
                interrupts.vblank = true;
                interrupts.stat |= self.stat_interrupt_enabled(4);
            } else if self.ly == 0 {
                self.window_line = 0;
                self.set_mode(PpuMode::OamScan);
                interrupts.stat |= self.stat_interrupt_enabled(5);
            }

            interrupts.stat |= self.update_lyc_flag();
        }

        if self.ly < VISIBLE_LINES {
            let mode_3_end_dot = self.mode_3_end_dot();
            let next_mode = if self.line_dots < MODE_2_DOTS {
                PpuMode::OamScan
            } else if self.line_dots < mode_3_end_dot {
                PpuMode::Drawing
            } else {
                PpuMode::HBlank
            };

            if next_mode != self.mode {
                self.set_mode(next_mode);
                interrupts.stat |= match next_mode {
                    PpuMode::HBlank => self.stat_interrupt_enabled(3),
                    PpuMode::OamScan => self.stat_interrupt_enabled(5),
                    PpuMode::VBlank => self.stat_interrupt_enabled(4),
                    PpuMode::Drawing => false,
                };
            }
        }

        interrupts
    }

    pub fn read_register(&self, address: u16) -> Option<u8> {
        match address {
            0xFF40 => Some(self.lcdc),
            0xFF41 => Some((self.stat & 0xFC) | self.mode as u8 | 0x80),
            0xFF42 => Some(self.scy),
            0xFF43 => Some(self.scx),
            0xFF44 => Some(self.ly),
            0xFF45 => Some(self.lyc),
            0xFF47 => Some(self.bgp),
            0xFF48 => Some(self.obp0),
            0xFF49 => Some(self.obp1),
            0xFF4A => Some(self.wy),
            0xFF4B => Some(self.wx),
            0xFF4F => Some(if self.cgb { 0xFE | self.vbk } else { 0xFF }),
            0xFF68 => Some(if self.cgb { self.bcps | 0x40 } else { 0xFF }),
            0xFF69 => Some(if self.cgb {
                self.bg_palette_ram[(self.bcps & 0x3F) as usize]
            } else {
                0xFF
            }),
            0xFF6A => Some(if self.cgb { self.ocps | 0x40 } else { 0xFF }),
            0xFF6B => Some(if self.cgb {
                self.obj_palette_ram[(self.ocps & 0x3F) as usize]
            } else {
                0xFF
            }),
            _ => None,
        }
    }

    pub fn set_cgb(&mut self, enabled: bool) {
        self.cgb = enabled;
    }

    pub fn write_register(&mut self, address: u16, value: u8) -> bool {
        match address {
            0xFF40 => {
                let was_enabled = self.lcd_enabled();
                self.lcdc = value;
                if was_enabled && !self.lcd_enabled() {
                    self.ly = 0;
                    self.line_dots = 0;
                    self.window_line = 0;
                    self.frame_ready = false;
                    self.set_mode(PpuMode::HBlank);
                    self.update_lyc_flag();
                } else if !was_enabled && self.lcd_enabled() {
                    self.ly = 0;
                    self.line_dots = 0;
                    self.window_line = 0;
                    self.set_mode(PpuMode::OamScan);
                    self.update_lyc_flag();
                }
                true
            }
            0xFF41 => {
                self.stat = (self.stat & 0x07) | (value & 0x78) | 0x80;
                true
            }
            0xFF42 => {
                self.scy = value;
                true
            }
            0xFF43 => {
                self.scx = value;
                true
            }
            0xFF44 => {
                self.ly = 0;
                self.line_dots = 0;
                self.window_line = 0;
                self.set_mode(if self.lcd_enabled() {
                    PpuMode::OamScan
                } else {
                    PpuMode::HBlank
                });
                self.update_lyc_flag();
                true
            }
            0xFF45 => {
                self.lyc = value;
                self.update_lyc_flag();
                true
            }
            0xFF47 => {
                self.bgp = value;
                true
            }
            0xFF48 => {
                self.obp0 = value;
                true
            }
            0xFF49 => {
                self.obp1 = value;
                true
            }
            0xFF4A => {
                self.wy = value;
                true
            }
            0xFF4B => {
                self.wx = value;
                true
            }
            0xFF4F => {
                if self.cgb {
                    self.vbk = value & 0x01;
                }
                true
            }
            0xFF68 => {
                self.bcps = value & 0xBF;
                true
            }
            0xFF69 => {
                if self.cgb {
                    let index = (self.bcps & 0x3F) as usize;
                    self.bg_palette_ram[index] = value;
                    if self.bcps & 0x80 != 0 {
                        self.bcps = 0x80 | (self.bcps.wrapping_add(1) & 0x3F);
                    }
                }
                true
            }
            0xFF6A => {
                self.ocps = value & 0xBF;
                true
            }
            0xFF6B => {
                if self.cgb {
                    let index = (self.ocps & 0x3F) as usize;
                    self.obj_palette_ram[index] = value;
                    if self.ocps & 0x80 != 0 {
                        self.ocps = 0x80 | (self.ocps.wrapping_add(1) & 0x3F);
                    }
                }
                true
            }
            _ => false,
        }
    }

    pub fn mode(&self) -> PpuMode {
        self.mode
    }

    pub fn framebuffer(&self) -> &[u32] {
        &self.framebuffer
    }

    pub fn take_frame_ready(&mut self) -> bool {
        let ready = self.frame_ready;
        self.frame_ready = false;
        ready
    }

    pub fn read_vram(&self, address: u16) -> Option<u8> {
        if self.mode == PpuMode::Drawing && self.lcd_enabled() {
            return Some(0xFF);
        }

        self.read_vram_raw(address)
    }

    pub fn write_vram(&mut self, address: u16, value: u8) -> bool {
        if self.mode == PpuMode::Drawing && self.lcd_enabled() {
            return address < VRAM_SIZE as u16;
        }

        self.write_vram_raw(address, value)
    }

    pub fn read_oam(&self, address: u16) -> Option<u8> {
        if matches!(self.mode, PpuMode::OamScan | PpuMode::Drawing) && self.lcd_enabled() {
            return Some(0xFF);
        }

        self.read_oam_raw(address)
    }

    pub fn write_oam(&mut self, address: u16, value: u8) -> bool {
        if matches!(self.mode, PpuMode::OamScan | PpuMode::Drawing) && self.lcd_enabled() {
            return address < OAM_SIZE as u16;
        }

        self.write_oam_raw(address, value)
    }

    pub fn read_vram_raw(&self, address: u16) -> Option<u8> {
        self.vram[self.vbk as usize].read(address as usize)
    }

    pub fn write_vram_raw(&mut self, address: u16, value: u8) -> bool {
        self.vram[self.vbk as usize].write(address as usize, value)
    }

    fn read_vram_bank(&self, bank: usize, address: usize) -> u8 {
        self.vram[bank & 1].read(address).unwrap_or(0)
    }

    pub fn write_vram_bank_raw(&mut self, bank: usize, address: u16, value: u8) -> bool {
        self.vram[bank & 1].write(address as usize, value)
    }

    pub fn vram_bank(&self) -> u8 {
        self.vbk & 0x01
    }

    pub fn read_oam_raw(&self, address: u16) -> Option<u8> {
        self.oam.read(address as usize)
    }

    pub fn write_oam_raw(&mut self, address: u16, value: u8) -> bool {
        self.oam.write(address as usize, value)
    }

    fn lcd_enabled(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    fn render_scanline(&mut self) {
        if self.ly as usize >= SCREEN_HEIGHT {
            return;
        }

        if !self.cgb && self.lcdc & 0x01 == 0 {
            self.clear_scanline();
            return;
        }

        let mut window_used = false;
        for x in 0..SCREEN_WIDTH {
            let (color_id, color, priority) = if self.window_pixel_visible(x) {
                window_used = true;
                self.render_window_pixel(x)
            } else {
                self.render_background_pixel(x)
            };

            let index = self.ly as usize * SCREEN_WIDTH + x;
            self.bg_color_ids[index] = color_id;
            self.bg_attr_priority[index] = priority;
            self.framebuffer[index] = color;
        }

        if window_used {
            self.window_line = self.window_line.wrapping_add(1);
        }

        self.render_sprites_on_scanline();
    }

    fn clear_scanline(&mut self) {
        let start = self.ly as usize * SCREEN_WIDTH;
        if start >= self.framebuffer.len() {
            return;
        }

        self.framebuffer[start..start + SCREEN_WIDTH].fill(DMG_COLORS[0]);
        self.bg_color_ids[start..start + SCREEN_WIDTH].fill(0);
        self.bg_attr_priority[start..start + SCREEN_WIDTH].fill(false);
        self.render_sprites_on_scanline();
    }

    fn render_background_pixel(&self, screen_x: usize) -> (u8, u32, bool) {
        let map_base = if self.lcdc & 0x08 != 0 {
            0x1C00
        } else {
            0x1800
        };
        let tile_data_unsigned = self.lcdc & 0x10 != 0;
        let y = self.ly.wrapping_add(self.scy);
        let x = (screen_x as u8).wrapping_add(self.scx);

        self.render_tile_pixel(
            map_base,
            tile_data_unsigned,
            x as usize,
            y as usize,
            self.bgp,
        )
    }

    fn render_window_pixel(&self, screen_x: usize) -> (u8, u32, bool) {
        let map_base = if self.lcdc & 0x40 != 0 {
            0x1C00
        } else {
            0x1800
        };
        let tile_data_unsigned = self.lcdc & 0x10 != 0;
        let window_x = if self.wx < 7 {
            screen_x + (7 - self.wx) as usize
        } else {
            screen_x - (self.wx - 7) as usize
        };
        self.render_tile_pixel(
            map_base,
            tile_data_unsigned,
            window_x,
            self.window_line as usize,
            self.bgp,
        )
    }

    fn render_tile_pixel(
        &self,
        map_base: usize,
        tile_data_unsigned: bool,
        x: usize,
        y: usize,
        palette: u8,
    ) -> (u8, u32, bool) {
        let tile_x = (x / 8) % TILE_MAP_SIZE;
        let tile_y = (y / 8) % TILE_MAP_SIZE;
        let map_offset = map_base + tile_y * TILE_MAP_SIZE + tile_x;
        let tile_id = self.read_vram_bank(0, map_offset);
        let tile_offset = tile_data_offset(tile_id, tile_data_unsigned);

        if self.cgb {
            let attr = self.read_vram_bank(1, map_offset);
            let cgb_palette = attr & 0x07;
            let bank = ((attr >> 3) & 0x01) as usize;
            let x_flip = attr & 0x20 != 0;
            let y_flip = attr & 0x40 != 0;
            let priority = attr & 0x80 != 0;
            let row = if y_flip { 7 - (y % 8) } else { y % 8 };
            let low = self.read_vram_bank(bank, tile_offset + row * 2);
            let high = self.read_vram_bank(bank, tile_offset + row * 2 + 1);
            let px = x % 8;
            let bit = if x_flip { px } else { 7 - px };
            let color_id = ((high >> bit) & 1) << 1 | ((low >> bit) & 1);
            (
                color_id,
                cgb_color(&self.bg_palette_ram, cgb_palette, color_id),
                priority,
            )
        } else {
            let col = x % 8;
            let row = y % 8;
            let low = self.read_vram_bank(0, tile_offset + row * 2);
            let high = self.read_vram_bank(0, tile_offset + row * 2 + 1);
            let bit = 7 - col;
            let color_id = ((high >> bit) & 1) << 1 | ((low >> bit) & 1);
            (color_id, palette_color(palette, color_id), false)
        }
    }

    fn window_pixel_visible(&self, screen_x: usize) -> bool {
        if self.lcdc & 0x20 == 0 || self.ly < self.wy || self.wx > 166 {
            return false;
        }

        let window_left = self.wx.saturating_sub(7) as usize;
        screen_x >= window_left
    }

    fn render_sprites_on_scanline(&mut self) {
        if self.lcdc & 0x02 == 0 || self.ly as usize >= SCREEN_HEIGHT {
            return;
        }

        let sprite_height = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let mut sprites = Vec::with_capacity(MAX_SPRITES_PER_LINE);

        for oam_index in 0..SPRITE_COUNT {
            let offset = oam_index * OAM_ENTRY_BYTES;
            let y = self.oam.read(offset).unwrap_or(0) as i16 - 16;
            let x = self.oam.read(offset + 1).unwrap_or(0) as i16 - 8;
            let tile = self.oam.read(offset + 2).unwrap_or(0);
            let attributes = self.oam.read(offset + 3).unwrap_or(0);
            let line = self.ly as i16 - y;

            if line < 0 || line >= sprite_height {
                continue;
            }

            sprites.push(Sprite {
                oam_index,
                x,
                y,
                tile,
                attributes,
                line: line as usize,
            });
            if sprites.len() == MAX_SPRITES_PER_LINE {
                break;
            }
        }

        if self.cgb {
            sprites.sort_by_key(|sprite| core::cmp::Reverse(sprite.oam_index));
        } else {
            sprites.sort_by(|a, b| b.x.cmp(&a.x).then_with(|| b.oam_index.cmp(&a.oam_index)));
        }

        for sprite in sprites {
            self.render_sprite_line(sprite, sprite_height as usize);
        }
    }

    fn render_sprite_line(&mut self, sprite: Sprite, sprite_height: usize) {
        let y_flip = sprite.attributes & 0x40 != 0;
        let x_flip = sprite.attributes & 0x20 != 0;
        let behind_bg = sprite.attributes & 0x80 != 0;
        let line = if y_flip {
            sprite_height - 1 - sprite.line
        } else {
            sprite.line
        };
        let tile = if sprite_height == 16 {
            sprite.tile & 0xFE
        } else {
            sprite.tile
        };
        let bank = if self.cgb {
            ((sprite.attributes >> 3) & 0x01) as usize
        } else {
            0
        };
        let tile_offset = tile as usize * TILE_BYTES + line * 2;
        let low = self.read_vram_bank(bank, tile_offset);
        let high = self.read_vram_bank(bank, tile_offset + 1);
        let master_priority = self.lcdc & 0x01 != 0;

        for pixel in 0..8 {
            let screen_x = sprite.x + pixel;
            if !(0..SCREEN_WIDTH as i16).contains(&screen_x) {
                continue;
            }

            let col = if x_flip {
                pixel as usize
            } else {
                7 - pixel as usize
            };
            let color_id = ((high >> col) & 1) << 1 | ((low >> col) & 1);
            if color_id == 0 {
                continue;
            }

            let index = self.ly as usize * SCREEN_WIDTH + screen_x as usize;
            let bg_wins = if self.cgb {
                master_priority
                    && self.bg_color_ids[index] != 0
                    && (self.bg_attr_priority[index] || behind_bg)
            } else {
                behind_bg && self.bg_color_ids[index] != 0
            };
            if bg_wins {
                continue;
            }

            let color = if self.cgb {
                cgb_color(&self.obj_palette_ram, sprite.attributes & 0x07, color_id)
            } else {
                let palette = if sprite.attributes & 0x10 != 0 {
                    self.obp1
                } else {
                    self.obp0
                };
                palette_color(palette, color_id)
            };
            self.framebuffer[index] = color;
        }
    }

    fn set_mode(&mut self, mode: PpuMode) {
        self.mode = mode;
        self.stat = (self.stat & !0x03) | mode as u8;
    }

    fn update_lyc_flag(&mut self) -> bool {
        if self.ly == self.lyc {
            let was_set = self.stat & 0x04 != 0;
            self.stat |= 0x04;
            !was_set && self.stat_interrupt_enabled(6)
        } else {
            self.stat &= !0x04;
            false
        }
    }

    fn stat_interrupt_enabled(&self, bit: u8) -> bool {
        self.stat & (1 << bit) != 0
    }

    fn mode_3_end_dot(&self) -> u32 {
        MODE_2_DOTS + MODE_3_BASE_DOTS + (self.scx & 0x07) as u32
    }
}

fn tile_data_offset(tile_id: u8, unsigned: bool) -> usize {
    if unsigned {
        tile_id as usize * TILE_BYTES
    } else {
        (0x1000isize + (tile_id as i8 as isize) * TILE_BYTES as isize) as usize
    }
}

fn palette_color(palette: u8, color_id: u8) -> u32 {
    let shade = (palette >> (color_id * 2)) & 0x03;
    DMG_COLORS[shade as usize]
}

fn cgb_color(palette_ram: &[u8; CGB_PALETTE_RAM_SIZE], palette: u8, color_id: u8) -> u32 {
    let base = (palette as usize % 8) * 8 + (color_id as usize) * 2;
    let rgb = palette_ram[base] as u16 | ((palette_ram[base + 1] as u16) << 8);
    let scale = |channel: u16| -> u32 {
        let c = (channel & 0x1F) as u32;
        (c << 3) | (c >> 2)
    };
    let r = scale(rgb);
    let g = scale(rgb >> 5);
    let b = scale(rgb >> 10);
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advance_to_hblank(ppu: &mut Ppu) {
        ppu.advance_cycles(ppu.mode_3_end_dot());
    }

    fn framebuffer_hash(framebuffer: &[u32]) -> u64 {
        framebuffer
            .iter()
            .fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
                let hash = hash ^ *pixel as u64;
                hash.wrapping_mul(0x0000_0100_0000_01B3)
            })
    }

    fn write_solid_tile(ppu: &mut Ppu, tile: u8, color_id: u8) {
        let low = if color_id & 0x01 != 0 { 0xFF } else { 0x00 };
        let high = if color_id & 0x02 != 0 { 0xFF } else { 0x00 };
        let base = tile as u16 * TILE_BYTES as u16;
        for row in 0..8 {
            ppu.write_vram_raw(base + row * 2, low);
            ppu.write_vram_raw(base + row * 2 + 1, high);
        }
    }

    #[test]
    fn default_ppu_has_framebuffer_and_oam_mode() {
        let ppu = Ppu::default();

        assert_eq!(ppu.mode(), PpuMode::OamScan);
        assert_eq!(ppu.framebuffer().len(), FRAMEBUFFER_PIXELS);
        assert_eq!(ppu.read_register(0xFF44), Some(0));
    }

    #[test]
    fn advances_through_visible_line_modes() {
        let mut ppu = Ppu::default();

        assert_eq!(ppu.mode(), PpuMode::OamScan);
        ppu.advance_cycles(80);
        assert_eq!(ppu.mode(), PpuMode::Drawing);
        ppu.advance_cycles(172);
        assert_eq!(ppu.mode(), PpuMode::HBlank);
        ppu.advance_cycles(204);
        assert_eq!(ppu.read_register(0xFF44), Some(1));
        assert_eq!(ppu.mode(), PpuMode::OamScan);
    }

    #[test]
    fn enters_vblank_at_line_144_and_latches_frame_ready() {
        let mut ppu = Ppu::default();
        let mut interrupts = PpuInterrupts::default();

        for _ in 0..VISIBLE_LINES {
            interrupts = ppu.advance_cycles(DOTS_PER_LINE);
        }

        assert_eq!(ppu.read_register(0xFF44), Some(144));
        assert_eq!(ppu.mode(), PpuMode::VBlank);
        assert!(interrupts.vblank);
        assert!(ppu.take_frame_ready());
        assert!(!ppu.take_frame_ready());
    }

    #[test]
    fn stat_lyc_interrupt_is_requested_only_on_new_match() {
        let mut ppu = Ppu::default();

        ppu.write_register(0xFF41, 0x40);
        ppu.write_register(0xFF45, 1);
        let interrupts = ppu.advance_cycles(DOTS_PER_LINE);
        let repeated = ppu.advance_cycles(4);

        assert!(interrupts.stat);
        assert!(!repeated.stat);
        assert_eq!(ppu.read_register(0xFF41).unwrap() & 0x04, 0x04);
    }

    #[test]
    fn stat_mode_interrupts_are_requested_on_enabled_transitions() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF41, 0x28);

        assert!(ppu.advance_cycles(80 + 172).stat);
        assert!(ppu.advance_cycles(204).stat);
    }

    #[test]
    fn scx_low_bits_extend_drawing_mode() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF43, 7);

        ppu.advance_cycles(MODE_2_DOTS + MODE_3_BASE_DOTS);
        assert_eq!(ppu.mode(), PpuMode::Drawing);
        ppu.advance_cycles(7);
        assert_eq!(ppu.mode(), PpuMode::HBlank);
    }

    #[test]
    fn disabling_lcd_resets_line_mode_and_frame_state() {
        let mut ppu = Ppu::default();

        ppu.advance_cycles(DOTS_PER_LINE);
        ppu.write_register(0xFF40, 0x00);

        assert_eq!(ppu.read_register(0xFF44), Some(0));
        assert_eq!(ppu.mode(), PpuMode::HBlank);
        assert!(!ppu.take_frame_ready());
    }

    #[test]
    fn stat_register_preserves_read_only_mode_and_lyc_bits() {
        let mut ppu = Ppu::default();

        ppu.write_register(0xFF41, 0xFF);

        assert_eq!(ppu.read_register(0xFF41), Some(0xFE));
    }

    #[test]
    fn vram_reads_and_writes_are_available_to_ppu() {
        let mut ppu = Ppu::default();

        advance_to_hblank(&mut ppu);
        assert!(ppu.write_vram(0x123, 0xAB));
        assert_eq!(ppu.read_vram(0x123), Some(0xAB));
        assert!(!ppu.write_vram(0x2000, 0x12));
        assert_eq!(ppu.read_vram(0x2000), None);
    }

    #[test]
    fn vram_is_restricted_during_drawing_mode() {
        let mut ppu = Ppu::default();
        ppu.write_vram_raw(0x100, 0xAB);
        ppu.advance_cycles(MODE_2_DOTS);

        assert_eq!(ppu.mode(), PpuMode::Drawing);
        assert_eq!(ppu.read_vram(0x100), Some(0xFF));
        assert!(ppu.write_vram(0x100, 0x12));
        assert_eq!(ppu.read_vram_raw(0x100), Some(0xAB));
    }

    #[test]
    fn oam_reads_and_writes_are_available_to_ppu_outside_restricted_modes() {
        let mut ppu = Ppu::default();

        advance_to_hblank(&mut ppu);
        assert!(ppu.write_oam(0x10, 0xAB));
        assert_eq!(ppu.read_oam(0x10), Some(0xAB));
        assert!(!ppu.write_oam(0xA0, 0x12));
        assert_eq!(ppu.read_oam(0xA0), None);
    }

    #[test]
    fn oam_is_restricted_during_oam_and_drawing_modes() {
        let mut ppu = Ppu::default();
        ppu.write_oam_raw(0x10, 0xAB);

        assert_eq!(ppu.read_oam(0x10), Some(0xFF));
        assert!(ppu.write_oam(0x10, 0x12));
        assert_eq!(ppu.read_oam_raw(0x10), Some(0xAB));
        ppu.advance_cycles(MODE_2_DOTS);
        assert_eq!(ppu.read_oam(0x10), Some(0xFF));
    }

    #[test]
    fn renders_background_tile_pixels_into_framebuffer() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x91);
        ppu.write_register(0xFF47, 0b11_10_01_00);
        ppu.write_vram_raw(0x1800, 1);
        ppu.write_vram_raw(TILE_BYTES as u16, 0b1000_0000);
        ppu.write_vram_raw((TILE_BYTES + 1) as u16, 0);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[1]);
        assert_eq!(ppu.framebuffer()[1], DMG_COLORS[0]);
    }

    #[test]
    fn background_rendering_uses_scroll_registers() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x91);
        ppu.write_register(0xFF43, 8);
        ppu.write_vram_raw(0x1801, 2);
        ppu.write_vram_raw((TILE_BYTES * 2) as u16, 0b1000_0000);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[3]);
    }

    #[test]
    fn background_can_use_alternate_tile_map_and_signed_tile_data() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x89);
        ppu.write_vram_raw(0x1C00, 0xFF);
        ppu.write_vram_raw(0x0FF0, 0x80);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[3]);
    }

    #[test]
    fn window_renders_from_wy_wx_and_selected_tile_map() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0xF1);
        ppu.write_register(0xFF4A, 0);
        ppu.write_register(0xFF4B, 7);
        ppu.write_register(0xFF47, 0b11_10_01_00);
        ppu.write_vram_raw(0x1C00, 3);
        ppu.write_vram_raw((TILE_BYTES * 3) as u16, 0x80);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[1]);
        assert_eq!(ppu.framebuffer()[1], DMG_COLORS[0]);
    }

    #[test]
    fn window_with_wx_less_than_seven_starts_partway_into_tile() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0xF1);
        ppu.write_register(0xFF4A, 0);
        ppu.write_register(0xFF4B, 0);
        ppu.write_register(0xFF47, 0b11_10_01_00);
        ppu.write_vram_raw(0x1C00, 1);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x01);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[1]);
        assert_eq!(ppu.framebuffer()[1], DMG_COLORS[0]);
    }

    #[test]
    fn window_line_counter_advances_only_on_visible_window_lines() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0xF1);
        ppu.write_register(0xFF4A, 1);
        ppu.write_register(0xFF4B, 7);
        ppu.write_register(0xFF47, 0b11_10_01_00);
        ppu.write_vram_raw(0x1C00, 1);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x80);
        ppu.write_vram_raw(TILE_BYTES as u16 + 2, 0xC0);
        ppu.write_vram_raw(TILE_BYTES as u16 + 3, 0xC0);

        ppu.advance_cycles(DOTS_PER_LINE);
        ppu.advance_cycles(DOTS_PER_LINE);
        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[SCREEN_WIDTH], DMG_COLORS[1]);
        assert_eq!(ppu.framebuffer()[SCREEN_WIDTH * 2], DMG_COLORS[3]);
    }

    #[test]
    fn renders_sprite_pixels_over_background() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x93);
        ppu.write_register(0xFF48, 0b11_10_01_00);
        ppu.write_vram_raw(TILE_BYTES as u16, 0b1000_0000);
        ppu.write_oam_raw(0, 16);
        ppu.write_oam_raw(1, 8);
        ppu.write_oam_raw(2, 1);
        ppu.write_oam_raw(3, 0);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[1]);
        assert_eq!(ppu.framebuffer()[1], DMG_COLORS[0]);
    }

    #[test]
    fn sprite_color_zero_is_transparent() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x93);
        ppu.write_vram_raw(0x1800, 2);
        ppu.write_vram_raw((TILE_BYTES * 2) as u16, 0x80);
        ppu.write_oam_raw(0, 16);
        ppu.write_oam_raw(1, 8);
        ppu.write_oam_raw(2, 1);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[3]);
    }

    #[test]
    fn sprite_priority_keeps_nonzero_background_in_front() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x93);
        ppu.write_vram_raw(0x1800, 2);
        ppu.write_vram_raw((TILE_BYTES * 2) as u16, 0x80);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x80);
        ppu.write_oam_raw(0, 16);
        ppu.write_oam_raw(1, 8);
        ppu.write_oam_raw(2, 1);
        ppu.write_oam_raw(3, 0x80);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[3]);
    }

    #[test]
    fn sprite_x_and_y_flip_reverse_pixels() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x93);
        ppu.write_vram_raw(TILE_BYTES as u16 + 14, 0x01);
        ppu.write_oam_raw(0, 16);
        ppu.write_oam_raw(1, 8);
        ppu.write_oam_raw(2, 1);
        ppu.write_oam_raw(3, 0x60);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[3]);
    }

    #[test]
    fn eight_by_sixteen_sprites_use_even_tile_number() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x97);
        ppu.write_vram_raw((TILE_BYTES * 2) as u16, 0x80);
        ppu.write_vram_raw((TILE_BYTES * 3) as u16, 0);
        ppu.write_oam_raw(0, 16);
        ppu.write_oam_raw(1, 8);
        ppu.write_oam_raw(2, 3);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[3]);
    }

    #[test]
    fn lower_x_sprite_has_priority_over_higher_x_sprite() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x93);
        ppu.write_register(0xFF48, 0b11_10_01_00);
        ppu.write_register(0xFF49, 0b11_10_01_00);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x80);
        ppu.write_vram_raw((TILE_BYTES * 2) as u16, 0x80);
        ppu.write_oam_raw(0, 16);
        ppu.write_oam_raw(1, 9);
        ppu.write_oam_raw(2, 1);
        ppu.write_oam_raw(4, 16);
        ppu.write_oam_raw(5, 8);
        ppu.write_oam_raw(6, 2);
        ppu.write_oam_raw(7, 0x10);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[1], DMG_COLORS[1]);
    }

    #[test]
    fn only_ten_sprites_are_selected_per_scanline() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x93);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x80);
        for sprite in 0..11 {
            let offset = sprite * OAM_ENTRY_BYTES;
            ppu.write_oam_raw(offset as u16, 16);
            ppu.write_oam_raw((offset + 1) as u16, (8 + sprite) as u8);
            ppu.write_oam_raw((offset + 2) as u16, 1);
        }

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[10], DMG_COLORS[0]);
    }

    #[test]
    fn visual_regression_background_window_and_sprites() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0xF3);
        ppu.write_register(0xFF42, 4);
        ppu.write_register(0xFF43, 3);
        ppu.write_register(0xFF4A, 24);
        ppu.write_register(0xFF4B, 23);
        ppu.write_register(0xFF47, 0b11_10_01_00);
        ppu.write_register(0xFF48, 0b11_10_01_00);
        ppu.write_register(0xFF49, 0b11_10_01_00);

        write_solid_tile(&mut ppu, 1, 1);
        write_solid_tile(&mut ppu, 2, 2);
        write_solid_tile(&mut ppu, 3, 3);
        for index in 0..0x400 {
            ppu.write_vram_raw(0x1800 + index as u16, if index % 2 == 0 { 1 } else { 2 });
            ppu.write_vram_raw(0x1C00 + index as u16, 3);
        }

        ppu.write_oam_raw(0, 40);
        ppu.write_oam_raw(1, 32);
        ppu.write_oam_raw(2, 3);
        ppu.write_oam_raw(3, 0);
        ppu.write_oam_raw(4, 40);
        ppu.write_oam_raw(5, 28);
        ppu.write_oam_raw(6, 2);
        ppu.write_oam_raw(7, 0x80);

        for _ in 0..VISIBLE_LINES {
            ppu.advance_cycles(DOTS_PER_LINE);
        }

        assert_eq!(framebuffer_hash(ppu.framebuffer()), 0x9a26_f37f_2123_4965);
    }

    #[test]
    fn visual_regression_bg_disabled_sprite_priority_is_ignored() {
        let mut ppu = Ppu::default();
        ppu.write_register(0xFF40, 0x92);
        ppu.write_register(0xFF48, 0b11_10_01_00);
        write_solid_tile(&mut ppu, 1, 3);
        ppu.write_oam_raw(0, 16);
        ppu.write_oam_raw(1, 8);
        ppu.write_oam_raw(2, 1);
        ppu.write_oam_raw(3, 0x80);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], DMG_COLORS[3]);
        assert_eq!(framebuffer_hash(ppu.framebuffer()), 0xfe44_45db_b72e_5b0d);
    }

    fn write_cgb_color(ppu: &mut Ppu, index_reg: u16, data_reg: u16, slot: u8, rgb555: u16) {
        ppu.write_register(index_reg, 0x80 | (slot * 2));
        ppu.write_register(data_reg, (rgb555 & 0xFF) as u8);
        ppu.write_register(data_reg, (rgb555 >> 8) as u8);
    }

    #[test]
    fn cgb_vram_banks_are_independent() {
        let mut ppu = Ppu::default();
        ppu.set_cgb(true);

        ppu.write_register(0xFF4F, 0);
        assert!(ppu.write_vram_raw(0x100, 0xAA));
        ppu.write_register(0xFF4F, 1);
        assert!(ppu.write_vram_raw(0x100, 0xBB));

        assert_eq!(ppu.read_vram_raw(0x100), Some(0xBB));
        assert_eq!(ppu.read_register(0xFF4F), Some(0xFF));
        ppu.write_register(0xFF4F, 0);
        assert_eq!(ppu.read_vram_raw(0x100), Some(0xAA));
        assert_eq!(ppu.read_register(0xFF4F), Some(0xFE));
    }

    #[test]
    fn cgb_palette_data_auto_increments_and_reads_back() {
        let mut ppu = Ppu::default();
        ppu.set_cgb(true);

        ppu.write_register(0xFF68, 0x80);
        for value in 0..8u8 {
            ppu.write_register(0xFF69, value);
        }

        ppu.write_register(0xFF68, 0x03);
        assert_eq!(ppu.read_register(0xFF69), Some(3));
        assert_eq!(ppu.read_register(0xFF68), Some(0x43));
    }

    #[test]
    fn cgb_background_uses_color_palette() {
        let mut ppu = Ppu::default();
        ppu.set_cgb(true);
        ppu.write_register(0xFF40, 0x91);
        write_cgb_color(&mut ppu, 0xFF68, 0xFF69, 1, 0x001F);
        ppu.write_vram_raw(0x1800, 1);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x80);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], 0xFFFF_0000);
    }

    #[test]
    fn cgb_bg_attribute_selects_palette() {
        let mut ppu = Ppu::default();
        ppu.set_cgb(true);
        ppu.write_register(0xFF40, 0x91);
        write_cgb_color(&mut ppu, 0xFF68, 0xFF69, 4 + 1, 0x03E0);
        ppu.write_vram_raw(0x1800, 1);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x80);
        ppu.write_register(0xFF4F, 1);
        ppu.write_vram_raw(0x1800, 0x01);
        ppu.write_register(0xFF4F, 0);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], 0xFF00_FF00);
    }

    #[test]
    fn cgb_bg_attribute_x_flip_mirrors_tile() {
        let mut ppu = Ppu::default();
        ppu.set_cgb(true);
        ppu.write_register(0xFF40, 0x91);
        write_cgb_color(&mut ppu, 0xFF68, 0xFF69, 1, 0x001F);
        ppu.write_vram_raw(0x1800, 1);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x80);
        ppu.write_register(0xFF4F, 1);
        ppu.write_vram_raw(0x1800, 0x20);
        ppu.write_register(0xFF4F, 0);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[7], 0xFFFF_0000);
        assert_eq!(ppu.framebuffer()[0], 0xFF00_0000);
    }

    #[test]
    fn cgb_sprite_priority_uses_oam_index_not_x() {
        let mut ppu = Ppu::default();
        ppu.set_cgb(true);
        ppu.write_register(0xFF40, 0x93);
        write_cgb_color(&mut ppu, 0xFF6A, 0xFF6B, 1, 0x001F);
        write_cgb_color(&mut ppu, 0xFF6A, 0xFF6B, 4 + 1, 0x03E0);
        ppu.write_vram_raw(TILE_BYTES as u16, 0x80);
        ppu.write_vram_raw((TILE_BYTES * 2) as u16, 0x01);
        ppu.write_oam_raw(0, 16);
        ppu.write_oam_raw(1, 8);
        ppu.write_oam_raw(2, 1);
        ppu.write_oam_raw(3, 0x00);
        ppu.write_oam_raw(4, 16);
        ppu.write_oam_raw(5, 1);
        ppu.write_oam_raw(6, 2);
        ppu.write_oam_raw(7, 0x01);

        ppu.advance_cycles(DOTS_PER_LINE);

        assert_eq!(ppu.framebuffer()[0], 0xFFFF_0000);
    }
}
