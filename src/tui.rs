//! Frame-Buffer + Diff-Flush für Terminal-Output.

use std::fmt::Write as FmtWrite;
use std::io::{self, Write};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: u8,
    pub bg: u8,
}

impl Cell {
    pub const EMPTY: Cell = Cell {
        ch: ' ',
        fg: 16,
        bg: 16,
    };

    pub fn new(ch: char, fg: u8, bg: u8) -> Self {
        Self { ch, fg, bg }
    }
}

pub struct FrameBuffer {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    prev: Vec<Cell>,
    force_redraw: bool,
}

impl FrameBuffer {
    pub fn new(cols: usize, rows: usize) -> Self {
        let n = cols * rows;
        Self {
            cols,
            rows,
            cells: vec![Cell::EMPTY; n],
            prev: vec![Cell::EMPTY; n],
            force_redraw: true,
        }
    }

    pub fn cols(&self) -> usize { self.cols }
    pub fn rows(&self) -> usize { self.rows }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        let n = cols * rows;
        self.cols = cols;
        self.rows = rows;
        self.cells = vec![Cell::EMPTY; n];
        self.prev = vec![Cell::EMPTY; n];
        self.force_redraw = true;
    }

    pub fn clear(&mut self) {
        for c in self.cells.iter_mut() {
            *c = Cell::EMPTY;
        }
    }

    /// 0-basiert. (x,y) außerhalb → noop.
    pub fn put(&mut self, x: usize, y: usize, cell: Cell) {
        if x < self.cols && y < self.rows {
            self.cells[y * self.cols + x] = cell;
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Cell {
        if x < self.cols && y < self.rows {
            self.cells[y * self.cols + x]
        } else {
            Cell::EMPTY
        }
    }

    /// Schreibt nur veränderte Zellen als ANSI-Sequenz. Beim ersten Aufruf
    /// (oder nach Resize) wird der gesamte Buffer komplett geschrieben.
    pub fn flush_diff(&mut self, out: &mut impl Write) -> io::Result<()> {
        let mut buf = String::with_capacity(self.cols * self.rows * 16);
        buf.push_str("\x1b[H"); // Home

        let mut cursor_y: Option<usize> = None;
        let mut cursor_x: Option<usize> = None;
        let mut last_fg: Option<u8> = None;
        let mut last_bg: Option<u8> = None;

        for y in 0..self.rows {
            for x in 0..self.cols {
                let idx = y * self.cols + x;
                let now = self.cells[idx];
                let was = self.prev[idx];
                if !self.force_redraw && now == was {
                    cursor_x = None; // Cursor weiter, aber wir haben nichts geschrieben
                    cursor_y = None;
                    continue;
                }
                // Cursor an (x, y) positionieren (1-basiert für ANSI)
                if cursor_x != Some(x) || cursor_y != Some(y) {
                    write!(&mut buf, "\x1b[{};{}H", y + 1, x + 1).unwrap();
                }
                if last_fg != Some(now.fg) {
                    write!(&mut buf, "\x1b[38;5;{}m", now.fg).unwrap();
                    last_fg = Some(now.fg);
                }
                if last_bg != Some(now.bg) {
                    write!(&mut buf, "\x1b[48;5;{}m", now.bg).unwrap();
                    last_bg = Some(now.bg);
                }
                buf.push(now.ch);
                cursor_x = Some(x + 1);
                cursor_y = Some(y);
            }
        }
        buf.push_str("\x1b[0m");

        out.write_all(buf.as_bytes())?;
        out.flush()?;

        self.prev.copy_from_slice(&self.cells);
        self.force_redraw = false;
        Ok(())
    }

    pub fn force_full_redraw(&mut self) {
        self.force_redraw = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_initializes_to_empty() {
        let fb = FrameBuffer::new(4, 3);
        assert_eq!(fb.cols(), 4);
        assert_eq!(fb.rows(), 3);
        for y in 0..3 {
            for x in 0..4 {
                assert_eq!(fb.get(x, y), Cell::EMPTY);
            }
        }
    }

    #[test]
    fn put_and_get_roundtrip() {
        let mut fb = FrameBuffer::new(4, 3);
        let c = Cell::new('@', 200, 17);
        fb.put(1, 2, c);
        assert_eq!(fb.get(1, 2), c);
    }

    #[test]
    fn put_out_of_bounds_is_noop() {
        let mut fb = FrameBuffer::new(4, 3);
        fb.put(99, 99, Cell::new('X', 10, 10));
        // Kein Crash, alle Zellen weiterhin leer
        for y in 0..3 {
            for x in 0..4 {
                assert_eq!(fb.get(x, y), Cell::EMPTY);
            }
        }
    }

    #[test]
    fn resize_shrinks_and_grows() {
        let mut fb = FrameBuffer::new(4, 3);
        fb.put(0, 0, Cell::new('A', 10, 10));
        fb.resize(2, 2);
        assert_eq!(fb.cols(), 2);
        assert_eq!(fb.rows(), 2);
        // Alle Zellen zurückgesetzt
        assert_eq!(fb.get(0, 0), Cell::EMPTY);
        fb.resize(8, 8);
        assert_eq!(fb.cols(), 8);
        assert_eq!(fb.get(0, 0), Cell::EMPTY);
    }

    #[test]
    fn clear_resets_all_cells() {
        let mut fb = FrameBuffer::new(3, 3);
        for x in 0..3 {
            fb.put(x, 1, Cell::new('#', 50, 100));
        }
        fb.clear();
        for x in 0..3 {
            assert_eq!(fb.get(x, 1), Cell::EMPTY);
        }
    }

    #[test]
    fn flush_first_time_writes_full_buffer() {
        let mut fb = FrameBuffer::new(2, 1);
        fb.put(0, 0, Cell::new('A', 100, 16));
        fb.put(1, 0, Cell::new('B', 200, 16));
        let mut out = Vec::new();
        fb.flush_diff(&mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains('A'));
        assert!(s.contains('B'));
        assert!(s.contains("38;5;100"));
        assert!(s.contains("38;5;200"));
    }

    #[test]
    fn flush_diff_only_writes_changes() {
        let mut fb = FrameBuffer::new(2, 1);
        fb.put(0, 0, Cell::new('A', 100, 16));
        fb.put(1, 0, Cell::new('B', 200, 16));
        let mut out1 = Vec::new();
        fb.flush_diff(&mut out1).unwrap();

        // Nur 1 Zelle geändert
        fb.put(1, 0, Cell::new('C', 200, 16));
        let mut out2 = Vec::new();
        fb.flush_diff(&mut out2).unwrap();
        let s2 = String::from_utf8(out2).unwrap();
        assert!(s2.contains('C'));
        assert!(!s2.contains('A'), "unchanged A should not be re-emitted: {:?}", s2);
    }

    #[test]
    fn resize_forces_full_redraw_next_flush() {
        let mut fb = FrameBuffer::new(2, 1);
        fb.put(0, 0, Cell::new('A', 100, 16));
        fb.flush_diff(&mut Vec::new()).unwrap();

        fb.resize(3, 1);
        fb.put(0, 0, Cell::new('A', 100, 16));
        let mut out = Vec::new();
        fb.flush_diff(&mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        // Erste Zelle (war identisch zu früher) muss neu geschrieben werden
        assert!(s.contains('A'));
    }
}
