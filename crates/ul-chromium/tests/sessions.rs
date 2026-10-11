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

/// O navegador de pé com o DevTools (`measure-navigation.sh` com `MODO=close`, `ss-none.html` e `about:blank`): o
/// `Session_` leva a entrada única da página e o `Browser.close` grava o `Tabs_` com o título.
mod live {
    use ul_chromium::sessions::{decode_entry, live_session, live_tabs};

    use super::fixture;

    /// Zera os dois instantes do fechamento no `Tabs_` (o comando 9 e o 4), que o oráculo sorteia entre 13 e 22 ms.
    fn without_close_times(mut tabs: Vec<u8>) -> Vec<u8> {
        tabs[27..35].fill(0);
        tabs[74..82].fill(0);
        tabs
    }

    fn utf16(units: &[u8]) -> String {
        String::from_utf16(&units.chunks(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>()).unwrap()
    }

    /// O conteúdo do primeiro comando `id` de um arquivo SNSS.
    fn command_of(file: &[u8], id: u8) -> Vec<u8> {
        let mut at = 8;
        while at + 3 <= file.len() {
            let size = usize::from(u16::from_le_bytes([file[at], file[at + 1]]));
            if file[at + 2] == id {
                return file[at + 3..at + 2 + size].to_vec();
            }
            at += 2 + size;
        }
        panic!("sem comando {id}");
    }

    #[test]
    fn the_page_entry_rebuilds_the_oracles_session_from_its_own_parameters() {
        let oracle = fixture("live-page-session");
        let parts = decode_entry(&command_of(&oracle, 6)).unwrap();
        assert_eq!((parts.index, parts.transition, parts.status, parts.url.as_str()), (0, 6, 200, "http://127.0.0.1:8000/ss-none.html"));
        // Os parâmetros sorteados saem do estado do oráculo: a sequência (dois i64 em 0x6C) e os dois GUIDs UTF-16.
        let sequence = i64::from_le_bytes(parts.state[0x6C..0x74].try_into().unwrap());
        let guids = [utf16(&parts.state[0x1D4..0x1D4 + 72]), utf16(&parts.state[0x234..0x234 + 72])];
        assert_eq!(live_session(&oracle, &parts.url, &guids, sequence, parts.time).unwrap(), oracle);
        // Sobre o molde `about:blank` o mesmo parâmetro troca só a entrada: o arquivo cresce 96 bytes (808 contra 712).
        let blank = fixture("live-blank-session");
        let rebuilt = live_session(&blank, &parts.url, &guids, sequence, parts.time).unwrap();
        assert_eq!(rebuilt.len(), blank.len() + 96);
        assert_eq!(rebuilt.len(), oracle.len());
    }

    #[test]
    fn the_tabs_file_of_a_page_matches_the_oracle_but_for_the_close_times() {
        let session = fixture("live-page-session");
        let oracle = fixture("live-page-tabs");
        // O comando 9 leva o instante do fechamento 17 ms antes dele.
        let window_time = i64::from_le_bytes(oracle[27..35].try_into().unwrap());
        let tabs = live_tabs(&session, "ss none", window_time + 17_000).unwrap();
        assert_eq!(without_close_times(tabs), without_close_times(oracle));
    }

    #[test]
    fn the_tabs_file_of_about_blank_matches_the_oracle_but_for_the_close_times() {
        let session = fixture("live-blank-session");
        let oracle = fixture("live-blank-tabs");
        let window_time = i64::from_le_bytes(oracle[27..35].try_into().unwrap());
        let tabs = live_tabs(&session, "", window_time + 17_000).unwrap();
        assert_eq!(without_close_times(tabs), without_close_times(oracle));
    }
}
