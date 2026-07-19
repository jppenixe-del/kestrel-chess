pub type Square = u8; // 0..=63, a1=0, b1=1, ..., h1=7, a2=8, ..., h8=63

pub const NO_SQUARE: Square = 64;

#[inline(always)]
pub fn sq(file: u8, rank: u8) -> Square {
    rank * 8 + file
}
#[inline(always)]
pub fn file_of(s: Square) -> u8 {
    s % 8
}
#[inline(always)]
pub fn rank_of(s: Square) -> u8 {
    s / 8
}

pub fn sq_name(s: Square) -> String {
    if s == NO_SQUARE {
        return "-".to_string();
    }
    let f = (b'a' + file_of(s)) as char;
    let r = (b'1' + rank_of(s)) as char;
    format!("{}{}", f, r)
}

pub fn parse_sq(s: &str) -> Square {
    let bytes = s.as_bytes();
    let file = bytes[0] - b'a';
    let rank = bytes[1] - b'1';
    sq(file, rank)
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    #[inline(always)]
    pub fn opp(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
    #[inline(always)]
    pub fn idx(self) -> usize {
        self as usize
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PieceType {
    Pawn = 0,
    Knight = 1,
    Bishop = 2,
    Rook = 3,
    Queen = 4,
    King = 5,
}

pub const ALL_PIECES: [PieceType; 6] = [
    PieceType::Pawn,
    PieceType::Knight,
    PieceType::Bishop,
    PieceType::Rook,
    PieceType::Queen,
    PieceType::King,
];

impl PieceType {
    #[inline(always)]
    pub fn idx(self) -> usize {
        self as usize
    }

    pub fn to_char(self, color: Color) -> char {
        let c = match self {
            PieceType::Pawn => 'p',
            PieceType::Knight => 'n',
            PieceType::Bishop => 'b',
            PieceType::Rook => 'r',
            PieceType::Queen => 'q',
            PieceType::King => 'k',
        };
        if color == Color::White {
            c.to_ascii_uppercase()
        } else {
            c
        }
    }

    pub fn value(self) -> i32 {
        match self {
            PieceType::Pawn => 100,
            PieceType::Knight => 320,
            PieceType::Bishop => 330,
            PieceType::Rook => 500,
            PieceType::Queen => 900,
            PieceType::King => 20000,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Piece {
    pub kind: PieceType,
    pub color: Color,
}
