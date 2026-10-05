// Porte pseudo-linus: janelas de texto em bytes. O `ptx` do GNU mede todas as larguras em bytes (um
// caractere multibyte conta pelos seus bytes), então o texto fica como `&[u8]` de ponta a ponta.

use std::ops::Range;

/// Um espaço em branco, como o `isspace` do C.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Um texto endereçado por byte, no qual os campos do `ptx` são recortados em janelas de palavras
/// inteiras. Os campos são intervalos, e não cópias, porque quem chama precisa saber quanto texto
/// sobrou de cada lado: é isso que decide se uma marca de truncamento é impressa.
pub(crate) struct Line<'a>(pub(crate) &'a [u8]);

impl Line<'_> {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn text(&self, range: Range<usize>) -> Vec<u8> {
        self.0[range].to_vec()
    }

    fn space_at(&self, index: usize) -> bool {
        is_space(self.0[index])
    }

    /// `range` sem o branco das duas pontas. Uma janela só de brancos vira um intervalo vazio em
    /// `range.start`, pra um campo feito só de espaço não ocupar largura nenhuma.
    pub(crate) fn trim(&self, range: Range<usize>) -> Range<usize> {
        let Range { mut start, mut end } = range;
        let floor = start;
        while start < end && self.space_at(start) {
            start += 1;
        }
        while floor < end && self.space_at(end - 1) {
            end -= 1;
        }
        // Num intervalo só de brancos os dois laços se cruzam: puxa `start` de volta pra o resultado
        // ser vazio e não invertido (que daria pânico ao fatiar).
        start.min(end)..end
    }

    /// Empurra `range.start` pra depois de uma palavra que ele corta ao meio, pra um campo nunca abrir
    /// num pedaço de palavra. Começo que já cai numa fronteira, ou no início da linha, fica onde está.
    fn align_start_to_word(&self, range: Range<usize>) -> Range<usize> {
        let Range { mut start, end } = range;
        if start == end || start == 0 || self.space_at(start) || self.space_at(start - 1) {
            return start..end;
        }
        while start < end && !self.space_at(start) {
            start += 1;
        }
        start..end
    }

    /// O espelho de [`Self::align_start_to_word`]: recua `range.end` pra fora de uma palavra cortada.
    /// Fim no fim da linha não cortou nada e fica.
    fn align_end_to_word(&self, range: Range<usize>) -> Range<usize> {
        let Range { start, mut end } = range;
        if start == end || end == self.len() || self.space_at(end - 1) || self.space_at(end) {
            return start..end;
        }
        while start < end && !self.space_at(end - 1) {
            end -= 1;
        }
        start..end
    }

    /// No máximo `width` bytes tirados da ponta direita de `range`, em palavras inteiras e sem branco
    /// nas pontas. Os campos à esquerda do keyword crescem pra esquerda a partir de uma borda fixa:
    /// é assim que são preenchidos.
    pub(crate) fn window_ending_at(&self, range: Range<usize>, width: usize) -> Range<usize> {
        let end = self.trim(range).end;
        let start = end.saturating_sub(width);
        self.trim(self.align_start_to_word(start..end))
    }

    /// No máximo `width` bytes tirados da ponta esquerda de `range`, em palavras inteiras. O começo
    /// fica exatamente onde quem chamou pediu: o campo depois do keyword encosta nele, então o branco
    /// ali faz parte da saída e tem que sobreviver.
    pub(crate) fn window_starting_at(&self, range: Range<usize>, width: usize) -> Range<usize> {
        let start = range.start;
        let end = range.end.min(start.saturating_add(width));
        let end = self.align_end_to_word(start..end).end;
        start..self.trim(start..end).end
    }

    /// Se o byte em `index` é branco.
    pub(crate) fn is_space_at(&self, index: usize) -> bool {
        self.space_at(index)
    }
}
