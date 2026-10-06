//! `FT_Outline` e as operações do `ftoutln.c` que o resto do porte usa.

use crate::calc::mul_fix;

pub const TAG_ON: u8 = 1;
pub const TAG_CUBIC: u8 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vector {
    pub x: i64,
    pub y: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Outline {
    pub points: Vec<Vector>,
    pub tags: Vec<u8>,
    /// Índice do último ponto de cada contorno.
    pub contours: Vec<usize>,
    /// `FT_OUTLINE_OVERLAP`: contornos que se sobrepõem, renderizados com superamostragem.
    pub overlap: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BBox {
    pub x_min: i64,
    pub y_min: i64,
    pub x_max: i64,
    pub y_max: i64,
}

/// Matriz 16.16 (`FT_Matrix`).
#[derive(Clone, Copy, Debug)]
pub struct Matrix {
    pub xx: i64,
    pub xy: i64,
    pub yx: i64,
    pub yy: i64,
}

impl Outline {
    /// `FT_Outline_Get_CBox`: caixa de todos os pontos, inclusive os de controle.
    pub fn cbox(&self) -> BBox {
        let Some(first) = self.points.first() else { return BBox::default() };
        let mut b = BBox { x_min: first.x, y_min: first.y, x_max: first.x, y_max: first.y };
        for p in &self.points[1..] {
            b.x_min = b.x_min.min(p.x);
            b.x_max = b.x_max.max(p.x);
            b.y_min = b.y_min.min(p.y);
            b.y_max = b.y_max.max(p.y);
        }
        b
    }

    pub fn translate(&mut self, dx: i64, dy: i64) {
        for p in &mut self.points {
            p.x = p.x.wrapping_add(dx);
            p.y = p.y.wrapping_add(dy);
        }
    }

    /// `FT_Outline_Transform` (`FT_Vector_Transform` em cada ponto).
    pub fn transform(&mut self, m: &Matrix) {
        for p in &mut self.points {
            *p = transform_vector(*p, m);
        }
    }
}

pub fn transform_vector(v: Vector, m: &Matrix) -> Vector {
    Vector {
        x: mul_fix(v.x, m.xx).wrapping_add(mul_fix(v.y, m.xy)),
        y: mul_fix(v.x, m.yx).wrapping_add(mul_fix(v.y, m.yy)),
    }
}
