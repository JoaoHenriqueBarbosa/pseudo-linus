//! Formatos e ordenação (sortformat.c): colunas `-o`/`-O`/`o`/`O`, `--sort`/`k`, o formato padrão
//! de cada estilo (`generate_sysv_list`) e os ajustes dos modificadores (`-j`, `-y`, `-c`, `-L`,
//! `-T`, `-M`).
//!
//! As listas do original são encadeadas de trás pra frente; aqui `format_list` e `f_cooked` ficam em
//! ordem de tela, e `s_cooked`/`sort_list` na ordem em que a ordenação é aplicada (a última chave
//! digitada é a primeira a ordenar e a primeira digitada é a principal).

use super::table::{search_aix_array, search_format_array, search_macro_array, search_shortsort_array};
use super::*;

impl Ps {
    /// `procps_pid_length`: largura do maior pid possível (`/proc/sys/kernel/pid_max`), 5 sem ele.
    pub(crate) fn pid_length(&mut self) -> i32 {
        if let Some(n) = self.pid_length_cache {
            return n;
        }
        let mut n = 5;
        if let Some(d) = proc::read_path("/proc/sys/kernel/pid_max") {
            // fgets lê até 23 caracteres; tira o '\n' do fim.
            let line: Vec<u8> = d.iter().copied().take(23).collect();
            let upto = line.iter().position(|b| *b == b'\n').map_or(line.len(), |p| p + 1);
            let mut len = upto as i32;
            if upto > 0 && line[upto - 1] == b'\n' {
                len -= 1;
            }
            n = len;
        }
        self.pid_length_cache = Some(n);
        n
    }

    /// `do_one_spec`: um especificador (uma coluna) ou uma macro (várias, na ordem da macro).
    /// `override_head` troca o cabeçalho (`pid=ID`). `None` se não existe.
    pub(super) fn do_one_spec(&mut self, spec: &[u8], override_head: Option<&[u8]>) -> Option<Vec<FNode>> {
        if let Some(fs) = search_format_array(spec) {
            let mut w1 = if fs.flags & CF_PIDMAX != 0 {
                let w = self.pid_length();
                w.max(fs.head.len() as i32)
            } else {
                fs.width
            };
            let name = match override_head {
                Some(o) => {
                    w1 = w1.max(o.len() as i32);
                    o.to_vec()
                }
                None => fs.head.as_bytes().to_vec(),
            };
            return Some(vec![FNode { name, pr: Some(fs.pr), width: w1, vendor: fs.vendor, flags: fs.flags }]);
        }
        let body = search_macro_array(spec)?;
        let mut list = Vec::new();
        for tok in body.split([',', ' ']).filter(|t| !t.is_empty()) {
            let mut one = self.do_one_spec(tok.as_bytes(), override_head)?;
            list.append(&mut one);
        }
        Some(list)
    }

    /// `O_wrap`: o formato do usuário de `-O`/`O` vai entre `pid` e as colunas padrão.
    fn o_wrap(&mut self, sfn: &mut SfNode, otype: u8) {
        let trailer = if otype == b'b' { "END_BSD" } else { "END_SYS5" };
        let mut pid = self.do_one_spec(b"pid", None).unwrap_or_default();
        let mut wrapped = Vec::new();
        wrapped.append(&mut pid);
        wrapped.append(&mut sfn.f_cooked);
        wrapped.extend(self.do_one_spec(trailer.as_bytes(), None).unwrap_or_default());
        sfn.f_cooked = wrapped;
    }

    /// Formato AIX (`%C %p`): descritores e texto fixo.
    fn aix_format_parse(&mut self, sfn: &mut SfNode) -> Result<(), String> {
        // Contagem de itens pela mesma máquina de estados do original (que conta um item a mais
        // depois de cada `%x`).
        let sf = sfn.sf.clone();
        let mut walk = 0usize;
        let at = |i: usize| -> u8 { sf.get(i).copied().unwrap_or(0) };
        let mut items = 0usize;
        let mut c = at(walk);
        walk += 1;
        enum St {
            Initial,
            GetText,
            GetMore,
            GetDesc,
        }
        let mut st = St::Initial;
        loop {
            match st {
                St::Initial => {
                    if c == b'%' {
                        st = St::GetDesc;
                    } else if c == 0 {
                        break;
                    } else {
                        st = St::GetText;
                    }
                }
                St::GetText => {
                    items += 1;
                    st = St::GetMore;
                }
                St::GetMore => {
                    c = at(walk);
                    walk += 1;
                    if c == b'%' {
                        st = St::GetDesc;
                    } else if c == b' ' {
                        st = St::GetMore;
                    } else if c != 0 {
                        return Err("improper AIX field descriptor".into());
                    } else {
                        break;
                    }
                }
                St::GetDesc => {
                    items += 1;
                    c = at(walk);
                    walk += 1;
                    if c != 0 && c != b' ' {
                        st = St::Initial;
                    } else {
                        return Err("missing AIX field descriptor".into());
                    }
                }
            }
        }
        let mut walk = 0usize;
        while items > 0 {
            items -= 1;
            if at(walk) == b'%' {
                walk += 1;
                if at(walk) == b'%' {
                    return Err("missing AIX field descriptor".into());
                }
                let code = at(walk);
                walk += 1;
                let Some((spec, head)) = search_aix_array(code) else {
                    return Err("unknown AIX field descriptor".into());
                };
                match self.do_one_spec(spec.as_bytes(), Some(head.as_bytes())) {
                    Some(mut nodes) => sfn.f_cooked.append(&mut nodes),
                    None => return Err("AIX field descriptor processing bug".into()),
                }
            } else {
                let rest = &sf[walk.min(sf.len())..];
                let len = rest.iter().position(|b| *b == b'%').unwrap_or(rest.len());
                let text = rest[..len].to_vec();
                walk += len;
                sfn.f_cooked.push(FNode { width: len.min(i32::MAX as usize) as i32, name: text, pr: None, vendor: AIX, flags: CF_PRINT_EVERY_TIME });
            }
        }
        self.already_parsed_format = true;
        Ok(())
    }

    /// Lista de colunas de `-o`: `pid,comm=Nome,user:12`. Se falha e há `%`, tenta como AIX.
    fn format_parse(&mut self, sfn: &mut SfNode) -> Result<(), String> {
        match self.format_parse_inner(sfn) {
            Ok(()) => Ok(()),
            Err(e) => {
                if sfn.sf.contains(&b'%') {
                    self.aix_format_parse(sfn)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn format_parse_inner(&mut self, sfn: &mut SfNode) -> Result<(), String> {
        const SEPS: &[u8] = b" ,\t\n";
        let mut buf = sfn.sf.clone();
        let mut need_item = true;
        let mut items = 0usize;
        if buf.is_empty() {
            return Err("improper format list".into());
        }
        for c in &buf {
            if SEPS.contains(c) {
                if need_item {
                    return Err("improper format list".into());
                }
                need_item = true;
            } else {
                if need_item {
                    items += 1;
                }
                need_item = false;
            }
        }
        if items == 0 {
            return Err("empty format list".into());
        }
        if need_item {
            buf.pop();
        }
        let mut walk: Option<usize> = Some(0);
        while items > 0 {
            items -= 1;
            let start = walk.ok_or_else(|| "please report this bug".to_string())?;
            let rest = &buf[start..];
            let sep = rest.iter().position(|c| SEPS.contains(c));
            // Com itens restantes, o separador termina o item; no último, o resto é o item inteiro.
            let item_end = match sep {
                Some(s) if items > 0 => s,
                _ => rest.len(),
            };
            let item = &rest[..item_end];
            let (spec_part, equal_part) = match item.iter().position(|c| *c == b'=') {
                Some(e) => (&item[..e], Some(&item[e + 1..])),
                None => (item, None),
            };
            let (spec, colon_part) = match spec_part.iter().position(|c| *c == b':') {
                Some(c) => (&spec_part[..c], Some(&spec_part[c + 1..])),
                None => (spec_part, None),
            };
            let mut width_override: Option<i32> = None;
            if let Some(cp) = colon_part {
                let all_digits = cp.iter().all(u8::is_ascii_digit);
                if !all_digits || cp.first() == Some(&b'0') || cp.is_empty() {
                    return Err("column widths must be unsigned decimal numbers".into());
                }
                let mut v: i64 = 0;
                for d in cp {
                    v = (v * 10 + i64::from(d - b'0')).min(i64::from(i32::MAX));
                }
                if v <= 0 {
                    return Err("column widths must be unsigned decimal numbers".into());
                }
                width_override = Some(v as i32);
            }
            let spec_owned = spec.to_vec();
            let equal_owned = equal_part.map(<[u8]>::to_vec);
            let nodes = self.do_one_spec(&spec_owned, equal_owned.as_deref());
            let Some(mut nodes) = nodes else {
                if self.errbuf.is_none() {
                    let mut m = format!("unknown user-defined format specifier \"{}\"", String::from_utf8_lossy(&spec_owned));
                    m.truncate(79);
                    self.errbuf = Some(m);
                }
                return Err(self.errbuf.clone().unwrap_or_default());
            };
            if let Some(w) = width_override {
                if nodes.len() > 1 {
                    return Err("can not set width for a macro (multi-column) format specifier".into());
                }
                nodes[0].width = w;
            }
            sfn.f_cooked.append(&mut nodes);
            walk = sep.filter(|_| items > 0).map(|s| start + s + 1);
        }
        self.already_parsed_format = true;
        Ok(())
    }

    /// `do_one_sort_spec`: `[+|-]especificador`.
    fn do_one_sort_spec(&mut self, spec: &[u8]) -> Option<SortNode> {
        let mut order = 1i8;
        let mut spec = spec;
        if spec.first() == Some(&b'-') {
            order = -1;
            spec = &spec[1..];
        } else if spec.first() == Some(&b'+') {
            spec = &spec[1..];
        }
        let fs = search_format_array(spec)?;
        Some(SortNode { sr: fs.sr, order })
    }

    /// `--sort`/`k`: lista de chaves separadas por vírgula, espaço, tab ou nova linha.
    fn long_sort_parse(&mut self, sfn: &mut SfNode) -> Result<(), String> {
        const SEPS: &[u8] = b" ,\t\n";
        let mut buf = sfn.sf.clone();
        let mut need_item = true;
        let mut items = 0usize;
        if buf.is_empty() {
            return Err("improper sort list".into());
        }
        for c in &buf {
            if SEPS.contains(c) {
                if need_item {
                    return Err("improper sort list".into());
                }
                need_item = true;
            } else {
                if need_item {
                    items += 1;
                }
                need_item = false;
            }
        }
        if items == 0 {
            return Err("empty sort list".into());
        }
        if need_item {
            buf.pop();
        }
        for tok in buf.split(|c| SEPS.contains(c)) {
            match self.do_one_sort_spec(tok) {
                Some(n) => sfn.s_cooked.insert(0, n),
                None => return Err("unknown sort specifier".into()),
            }
        }
        self.already_parsed_sort = true;
        Ok(())
    }

    /// `verify_short_sort`: o argumento de `O` é uma ordenação curta (`+p-r`)?
    fn verify_short_sort(&self, arg: &[u8]) -> Result<(), String> {
        const ALL: &[u8] = b"CGJKMNPRSTUcfgjkmnoprstuvy+-";
        if !arg.iter().all(|c| ALL.contains(c)) {
            return Err("bad sorting code".into());
        }
        let mut seen = [false; 256];
        let mut i = 0;
        while i < arg.len() {
            let c = arg[i];
            match c {
                b'+' | b'-' => {
                    let nx = arg.get(i + 1).copied().unwrap_or(0);
                    if nx == 0 || nx == b'+' || nx == b'-' {
                        return Err("bad sorting code".into());
                    }
                }
                _ => {
                    if c == b'P' && self.forest_type != 0 {
                        return Err("PPID sort and forest output conflict".into());
                    }
                    if seen[c as usize] {
                        return Err("bad sorting code".into());
                    }
                    seen[c as usize] = true;
                }
            }
            i += 1;
        }
        Ok(())
    }

    fn short_sort_parse(&mut self, sfn: &mut SfNode) -> Result<(), String> {
        let mut direction = 1i8;
        for &c in sfn.sf.clone().iter() {
            match c {
                b'+' => direction = 1,
                b'-' => direction = -1,
                _ => {
                    let spec = search_shortsort_array(c).ok_or_else(|| "unknown sort specifier".to_string())?;
                    let mut node = self.do_one_sort_spec(spec.as_bytes()).ok_or_else(|| "unknown sort specifier".to_string())?;
                    node.order = direction;
                    sfn.s_cooked.insert(0, node);
                    direction = 0;
                }
            }
        }
        self.already_parsed_sort = true;
        Ok(())
    }

    /// `parse_O_option`: interpreta as opções deferidas na ordem da linha de comando.
    fn parse_o_options(&mut self) -> Result<(), String> {
        let mut list = std::mem::take(&mut self.sf_list);
        let mut result = Ok(());
        for sfn in list.iter_mut() {
            if let Err(e) = self.parse_o_option(sfn) {
                result = Err(e);
                break;
            }
        }
        self.sf_list = list;
        result
    }

    fn parse_o_option(&mut self, sfn: &mut SfNode) -> Result<(), String> {
        match sfn.code {
            SF_B_O | SF_G_FORMAT | SF_U_O => {
                self.format_parse(sfn)?;
                self.already_parsed_format = true;
                Ok(())
            }
            SF_U_O_UP => {
                if self.already_parsed_format {
                    return Err("option -O can not follow other format options".into());
                }
                self.format_parse(sfn)?;
                self.already_parsed_format = true;
                self.o_wrap(sfn, b'u');
                Ok(())
            }
            SF_B_O_UP => {
                let mut err = if self.have_gnu_sort || self.already_parsed_sort {
                    Some("multiple sort options".to_string())
                } else {
                    self.verify_short_sort(&sfn.sf).err()
                };
                if err.is_none() {
                    let _ = self.short_sort_parse(sfn);
                    self.already_parsed_sort = true;
                    return Ok(());
                }
                if self.already_parsed_format {
                    return Err("option O is neither first format nor sort order".into());
                }
                if self.format_parse(sfn).is_ok() {
                    self.already_parsed_format = true;
                    self.o_wrap(sfn, b'b');
                    return Ok(());
                }
                Err(err.take().unwrap_or_default())
            }
            SF_G_SORT | SF_B_M => {
                let r = if self.already_parsed_sort { Err("multiple sort options".to_string()) } else { self.long_sort_parse(sfn) };
                self.already_parsed_sort = true;
                r
            }
            _ => Err("please report this bug".into()),
        }
    }

    /// Guarda a opção para interpretar depois (`defer_sf_option`).
    pub(super) fn defer_sf_option(&mut self, arg: &[u8], source: i32) {
        self.sf_list.push(SfNode { sf: arg.to_vec(), code: source, s_cooked: Vec::new(), f_cooked: Vec::new() });
        if source == SF_G_SORT {
            self.have_gnu_sort = true;
        }
    }

    pub(super) fn reset_sortformat(&mut self) {
        self.sf_list.clear();
        self.format_list.clear();
        self.sort_list.clear();
        self.have_gnu_sort = false;
        self.already_parsed_sort = false;
        self.already_parsed_format = false;
    }

    /// `fmt_add_after`: insere depois da primeira coluna de cabeçalho `findme`.
    fn fmt_add_after(&mut self, findme: &str, putme: Vec<FNode>) -> bool {
        match self.format_list.iter().position(|n| n.name == findme.as_bytes()) {
            Some(i) => {
                for (k, n) in putme.into_iter().enumerate() {
                    self.format_list.insert(i + 1 + k, n);
                }
                true
            }
            None => false,
        }
    }

    /// `fmt_delete`: tira a primeira coluna de cabeçalho `findme`.
    fn fmt_delete(&mut self, findme: &str) -> bool {
        match self.format_list.iter().position(|n| n.name == findme.as_bytes()) {
            Some(i) => {
                self.format_list.remove(i);
                true
            }
            None => false,
        }
    }

    fn spec(&mut self, spec: &str) -> Vec<FNode> {
        self.do_one_spec(spec.as_bytes(), None).unwrap_or_default()
    }

    /// `generate_sysv_list`: o formato padrão SysV, montado de trás pra frente como no original.
    fn generate_sysv_list(&mut self) -> Result<(), String> {
        let ff = self.format_flags;
        let fm = self.format_modifiers;
        let per = self.personality;
        if fm & FM_Y != 0 && ff & FF_UL == 0 {
            return Err("modifier -y without format -l makes no sense".into());
        }
        // Cada `push` é um PUSH do original; no fim a ordem de tela é a inversa.
        let mut pushes: Vec<Vec<FNode>> = Vec::new();
        if self.prefer_bsd_defaults {
            if ff != 0 {
                pushes.push(self.spec("cmd"));
            } else {
                pushes.push(self.spec("args"));
            }
            pushes.push(self.spec("bsdtime"));
            if ff & FF_UL == 0 {
                pushes.push(self.spec("stat"));
            }
        } else {
            if ff & FF_UF != 0 {
                pushes.push(self.spec("cmd"));
            } else {
                pushes.push(self.spec("ucmd"));
            }
            pushes.push(self.spec("time"));
        }
        pushes.push(self.spec("tname"));
        if ff & FF_UF != 0 {
            pushes.push(self.spec("stime"));
        }
        if fm & FM_F != 0 {
            if fm & FM_P == 0 {
                pushes.push(self.spec("psr"));
            }
            if !(ff & FF_UL != 0 && fm & FM_Y != 0) {
                pushes.push(self.spec("rss"));
            }
        }
        if ff & FF_UL != 0 {
            pushes.push(self.spec("wchan"));
        }
        if ff & FF_UL != 0 && fm & FM_Y == 0 && per & PER_IRIX_L != 0 {
            pushes.push(self.spec("sgi_rss"));
            pushes.push(vec![FNode { width: 1, name: b":".to_vec(), pr: None, vendor: AIX, flags: CF_PRINT_EVERY_TIME }]);
        }
        if fm & FM_F != 0 || ff & FF_UL != 0 {
            pushes.push(self.spec("sz"));
        }
        if ff & FF_UL != 0 {
            if fm & FM_Y != 0 {
                pushes.push(self.spec("rss"));
            } else if per & (PER_ZAP_ADDR | PER_IRIX_L) != 0 {
                pushes.push(self.spec("sgi_p"));
            } else {
                pushes.push(self.spec("addr_1"));
            }
        }
        if fm & FM_C != 0 {
            pushes.push(self.spec("pri"));
            pushes.push(self.spec("class"));
        } else if ff & FF_UL != 0 {
            pushes.push(self.spec("ni"));
            if per & PER_IRIX_L != 0 {
                pushes.push(self.spec("priority"));
            } else {
                pushes.push(self.spec("opri"));
            }
        }
        if self.thread_flags & TF_U_L != 0 && ff & FF_UF != 0 {
            pushes.push(self.spec("nlwp"));
        }
        if ff & (FF_UF | FF_UL) != 0 && fm & FM_C == 0 {
            pushes.push(self.spec("c"));
        }
        if fm & FM_P != 0 {
            pushes.push(self.spec("psr"));
        }
        if self.thread_flags & TF_U_L != 0 {
            pushes.push(self.spec("lwp"));
        }
        if fm & FM_J != 0 {
            pushes.push(self.spec("sid"));
            pushes.push(self.spec("pgid"));
        }
        if ff & (FF_UF | FF_UL) != 0 {
            pushes.push(self.spec("ppid"));
        }
        if self.thread_flags & TF_U_T != 0 {
            pushes.push(self.spec("spid"));
        }
        pushes.push(self.spec("pid"));
        if ff & FF_UF != 0 {
            if per & PER_SANE_USER != 0 {
                pushes.push(self.spec("user"));
            } else {
                pushes.push(self.spec("uid_hack"));
            }
        } else if ff & FF_UL != 0 {
            pushes.push(self.spec("uid"));
        }
        if ff & FF_UL != 0 {
            pushes.push(self.spec("s"));
            if fm & FM_Y == 0 {
                pushes.push(self.spec("f"));
            }
        }
        if fm & FM_M != 0 {
            pushes.push(self.spec("label"));
        }
        self.format_list = pushes.into_iter().rev().flatten().collect();
        Ok(())
    }

    /// `process_sf_options`: junta as listas deferidas, escolhe o formato padrão e aplica os
    /// modificadores.
    pub(super) fn process_sf_options(&mut self) -> Result<(), String> {
        if !self.sf_list.is_empty() {
            self.parse_o_options()?;
        }
        // Mescla os formatos (ordem da linha de comando) e as ordenações.
        let list = std::mem::take(&mut self.sf_list);
        let mut sorts: Vec<SortNode> = Vec::new();
        for sfn in list.iter() {
            self.format_list.extend(sfn.f_cooked.iter().cloned());
        }
        for sfn in list.iter().rev() {
            if !sfn.s_cooked.is_empty() {
                let mut chain = sfn.s_cooked.clone();
                chain.extend(sorts);
                sorts = chain;
            }
        }
        self.sf_list = list;
        self.sort_list = sorts;
        if !self.sort_list.is_empty() && self.thread_flags & TF_NO_SORT != 0 {
            return Err("tell <procps@freelists.org> what you expected".into());
        }
        // Sem nada, o $PS_FORMAT vale antes do padrão.
        if self.format_flags == 0 && self.format_modifiers == 0 && self.format_list.is_empty() {
            if let Some(tmp) = getenv("PS_FORMAT").filter(|v| !v.is_empty()) {
                if self.thread_flags & TF_MUST_USE != 0 {
                    return Err("tell <procps@freelists.org> what you want (-L/-T, -m/m/H, and $PS_FORMAT)".into());
                }
                let mut sfn = SfNode { sf: tmp, code: 0, s_cooked: Vec::new(), f_cooked: Vec::new() };
                match self.format_parse(&mut sfn) {
                    Ok(()) => {
                        self.format_list.extend(sfn.f_cooked);
                        return Ok(());
                    }
                    Err(e) => io::eprint(format!("warning: $PS_FORMAT ignored. ({e})\n")),
                }
            }
        }
        if !self.format_list.is_empty() {
            if self.format_flags != 0 {
                return Err("conflicting format options".into());
            }
            if self.format_modifiers != 0 {
                return Err("can not use output modifiers with user-defined output".into());
            }
            if self.thread_flags & TF_MUST_USE != 0 {
                return Err("-L/-T with H/m/-m and -o/-O/o/O is nonsense".into());
            }
            return Ok(());
        }
        let spec: Option<&'static str> = match self.format_flags {
            0 => None,
            x if x == FF_UF | FF_UL => self.sysv_fl_format,
            x if x == FF_UF => self.sysv_f_format,
            x if x == FF_UL => self.sysv_l_format,
            x if x == FF_UJ => self.sysv_j_format,
            x if x == FF_UJ | FF_UL => Some("RD_lj"),
            x if x == FF_UJ | FF_UF => Some("RD_fj"),
            x if x == FF_BJ => self.bsd_j_format,
            x if x == FF_BL => self.bsd_l_format,
            x if x == FF_BS => self.bsd_s_format,
            x if x == FF_BU => self.bsd_u_format,
            x if x == FF_BV => self.bsd_v_format,
            x if x == FF_LX => Some("OL_X"),
            x if x == FF_LM => Some("OL_m"),
            x if x == FF_FC => Some("FLASK_context"),
            _ => return Err("conflicting format options".into()),
        };
        match spec {
            // Sem formato nomeado, a lista SysV já sai pronta (o original retorna aqui, sem os
            // ajustes de modificadores abaixo).
            None => return self.generate_sysv_list(),
            Some(s) => {
                let nodes = self.spec(s);
                self.format_list.extend(nodes);
            }
        }
        // Ajustes dos modificadores.
        let fm = self.format_modifiers;
        let ff = self.format_flags;
        if fm & FM_J != 0 {
            let pgid = self.spec("pgid");
            if !self.fmt_add_after("PPID", pgid.clone()) && !self.fmt_add_after("PID", pgid) {
                return Err("internal error: no PID or PPID for -j option".into());
            }
            let sid = self.spec("sid");
            if !self.fmt_add_after("PGID", sid) {
                return Err("lost my PGID".into());
            }
        }
        if fm & FM_Y != 0 {
            self.fmt_delete("F");
            let rss = self.spec("rss");
            if self.fmt_add_after("ADDR", rss) {
                self.fmt_delete("ADDR");
            }
        }
        if fm & FM_C != 0 {
            for n in ["%CPU", "CPU", "CP", "C"] {
                self.fmt_delete(n);
            }
            self.fmt_delete("NI");
            let class = self.spec("class");
            if !self.fmt_add_after("PRI", class) {
                return Err("internal error: no PRI for -c option".into());
            }
            self.fmt_delete("PRI");
            let pri = self.spec("pri");
            if !self.fmt_add_after("CLS", pri) {
                return Err("lost my CLS".into());
            }
        }
        if self.thread_flags & TF_U_T != 0 {
            let spid = self.spec("spid");
            if !self.fmt_add_after("PID", spid) && self.thread_flags & TF_MUST_USE != 0 {
                return Err("-T with H/-m/m but no PID for SPID to follow".into());
            }
        }
        if self.thread_flags & TF_U_L != 0 {
            let lwp = self.spec("lwp");
            let mut done = false;
            for n in ["SID", "SESS", "PGID", "PGRP", "PPID", "PID"] {
                if self.fmt_add_after(n, lwp.clone()) {
                    done = true;
                    break;
                }
            }
            if !done && self.thread_flags & TF_MUST_USE != 0 {
                return Err("-L with H/-m/m but no PID/PGID/SID/SESS for NLWP to follow".into());
            }
            let nlwp = self.spec("nlwp");
            self.fmt_add_after("%CPU", nlwp);
        }
        if fm & FM_M != 0 {
            let label = self.spec("label");
            for (k, n) in label.into_iter().enumerate() {
                self.format_list.insert(k, n);
            }
        }
        if self.personality & PER_ZAP_ADDR != 0 && ff & FF_UL != 0 {
            let p = self.spec("sgi_p");
            if self.fmt_add_after("ADDR", p) {
                self.fmt_delete("ADDR");
            }
        }
        if self.personality & PER_SANE_USER != 0 && ff & FF_UF != 0 {
            let u = self.spec("user");
            if self.fmt_add_after("UID", u) {
                self.fmt_delete("UID");
            }
        }
        Ok(())
    }
}
