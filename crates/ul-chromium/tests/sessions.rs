//! Os arquivos de sessão (`Session_*` e `Tabs_*`) contra os que o Chromium 154 do oráculo gravou na saída do
//! `--dump-dom` de `testbench/corpus/headless-storage/ss-none*.html` (`measure-navigation.sh`): os mesmos parâmetros
//! (id da janela, GUIDs, sequência, tempos) têm de reproduzir os bytes, com o endereço de 34 caracteres do molde e com
//! um de 66, que muda o tamanho da matriz do mojo e os ponteiros que a atravessam.

use ul_chromium::sessions::{session_file, tabs_file, Snapshot};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/sessions/{name}.snss", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn guids(list: [&str; 4]) -> [[String; 2]; 2] {
    [[list[0].to_owned(), list[1].to_owned()], [list[2].to_owned(), list[3].to_owned()]]
}

fn short() -> Snapshot {
    Snapshot {
        window_id: 1_713_707_349,
        tab_guid: "f4a670cd_9db9_4330_b920_80144f63e045".to_owned(),
        created: 13_436_159_414_213_568,
        blank_time: 13_436_159_414_224_519,
        page_time: 13_436_159_414_423_740,
        page_end: 13_436_159_414_450_084,
        closed: 13_436_159_414_450_088,
        tabs_time: 13_436_159_414_449_279,
        guids: guids(["1e36173c-6b07-4dd3-8601-d92c2b6dbce1", "15c60ffd-e649-4919-a641-5d19d06e6c83", "b9cf18a4-5e8e-45f1-8794-c5a43accb926", "f528c112-5567-4102-9c5c-b082f1ed85e3"]),
        sequence: 0x65d875c815c45,
        url: "http://127.0.0.1:8000/ss-none.html".to_owned(),
        title: "ss none".to_owned(),
    }
}

fn long() -> Snapshot {
    Snapshot {
        window_id: 1_213_823_108,
        tab_guid: "fdf7b7e9_4dfc_455f_9db1_61b060e3f538".to_owned(),
        created: 13_436_165_402_691_483,
        blank_time: 13_436_165_402_700_278,
        page_time: 13_436_165_402_827_552,
        page_end: 13_436_165_402_852_411,
        closed: 13_436_165_402_852_417,
        tabs_time: 13_436_165_402_851_345,
        guids: guids(["30d62d26-81ad-4992-8b25-d6ee4d6fd031", "b6ca584f-aabb-4533-9d98-5dcab2213406", "2c06d249-e1e5-4e85-960c-4a3e039df731", "8853eeea-a680-4460-a9e9-7915a58bfba0"]),
        sequence: 0x65d88c172561d,
        url: "http://127.0.0.1:8000/ss-none-with-a-much-longer-name-for-the-session-state.html".to_owned(),
        title: "ss none".to_owned(),
    }
}

#[test]
fn the_template_url_gives_the_oracles_files() {
    assert_eq!(session_file(&short()), fixture("short-session"));
    assert_eq!(tabs_file(&short()), fixture("short-tabs"));
}

#[test]
fn a_longer_url_moves_the_array_and_the_pointers_that_cross_it() {
    assert_eq!(session_file(&long()), fixture("long-session"));
    assert_eq!(tabs_file(&long()), fixture("long-tabs"));
}
