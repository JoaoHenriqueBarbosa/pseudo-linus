//! A quinta coluna fatorada dos goldens mapeados: a montagem das corridas (`prelude.runs ++ tails[i] ++ sufixo`) e o
//! `PositionMap` que sai dela têm de ser os mesmos que a coluna completa de antes dava.
mod common;

use common::{parse_preludes, preludes_from_json, ProgramMeta};
use zjsc::parser::position_map::PositionMap;

#[test]
fn factored_meta_joins_prelude_runs_tail_and_suffix() {
    let json = "[\n{\"text\":\"a\\n\\\"b\\\"\\n\",\"runs\":[1,1,0,-2,2,1,0,-3],\"tails\":[[],[2,9,0,-4]]}\n]\n";
    let preludes = parse_preludes(json);
    assert_eq!(preludes.len(), 1);
    assert_eq!(preludes[0].text, "a\n\"b\"\n");
    assert_eq!(preludes_from_json(json), vec!["a\n\"b\"\n".to_string()]);

    // Cauda 0: o prelúdio só tem as corridas comuns; o sufixo (linha 3 em diante) acrescenta duas.
    let plain = ProgramMeta::parse_factored("[1,0,3,1,2,0,5,3,2,0]", &preludes[0]);
    assert_eq!(plain.mode, 1);
    assert_eq!(plain.position_runs, [1, 1, 0, -2, 2, 1, 0, -3, 3, 1, 2, 0, 5, 3, 2, 0]);
    // Cauda 1: uma corrida a mais dentro do prelúdio; o sufixo não traz nenhuma.
    let tailed = ProgramMeta::parse_factored("[0,1]", &preludes[0]);
    assert_eq!(tailed.position_runs, [1, 1, 0, -2, 2, 1, 0, -3, 2, 9, 0, -4]);
    // Índice -1: linha sem corrida nenhuma (o caso `[]` de `positionRuns`).
    let none = ProgramMeta::parse_factored("[2,-1]", &preludes[0]);
    assert_eq!((none.mode, none.position_runs.len()), (2, 0));

    // O mapa montado é o da lista completa: a última corrida que cobre a posição vence.
    let map = PositionMap::new(&plain.position_runs, 0).unwrap();
    let whole = PositionMap::new(&[1, 1, 0, -2, 2, 1, 0, -3, 3, 1, 2, 0, 5, 3, 2, 0], 0).unwrap();
    assert_eq!(map, whole);
    assert_eq!(map.map(1, 5), (1, 3));
    assert_eq!(map.map(2, 8), (2, 5));
    assert_eq!(map.map(4, 2), (6, 2));
    assert_eq!(map.map(5, 3), (7, 3));
}

#[test]
fn string_entries_stay_plain_preludes() {
    let preludes = parse_preludes("[\n\"x\\n\",\n\"y\"\n]\n");
    assert_eq!((preludes[0].text.as_str(), preludes[1].text.as_str()), ("x\n", "y"));
    assert!(preludes[0].runs.is_empty() && preludes[0].tails.is_empty());
}

/// Todas as linhas dos goldens mapeados montam um mapa válido (múltiplo de quatro, índice de cauda existente).
#[test]
fn mapped_goldens_assemble_valid_maps() {
    let goldens: [(&str, &str); 7] = [
        (include_str!("golden/iterator_bun.tsv"), include_str!("golden/iterator.preludes.json")),
        (include_str!("golden/string_unicode_bun.tsv"), include_str!("golden/string_unicode.preludes.json")),
        (include_str!("golden/reflect_bun.tsv"), include_str!("golden/reflect.preludes.json")),
        (include_str!("golden/array_bun.tsv"), include_str!("golden/array.preludes.json")),
        (include_str!("golden/array_combo_bun.tsv"), include_str!("golden/array_combo.preludes.json")),
        (include_str!("golden/array_edge_bun.tsv"), include_str!("golden/array_edge.preludes.json")),
        (include_str!("golden/collections_bun.tsv"), include_str!("golden/collections.preludes.json")),
    ];
    for (tsv, preludes) in goldens {
        let preludes = parse_preludes(preludes);
        for line in tsv.lines().filter(|line| !line.is_empty()) {
            let columns: Vec<&str> = line.split('\t').collect();
            let Some(column) = columns.get(4) else { continue };
            let prelude = &preludes[columns[2].parse::<usize>().expect("índice do prelúdio")];
            let meta = ProgramMeta::parse_factored(column, prelude);
            assert!(PositionMap::new(&meta.position_runs, 0).is_some());
        }
    }
}
