//! O banco de regras do `file` 5.46 do Debian 13, embutido no binário.
//!
//! Os fragmentos ficam em `crates/ul-misc/vendor/file-5.46/magdir`: o `magic/Magdir` do tarball
//! original do file 5.46 com os patches do pacote Debian 5.46-5 (zip, msdos, sgml, att3b e
//! printer) e os arquivos `Header`, `Localstuff` e `debian-extra-magic`. Compilados com
//! `file -C -m magic` (diretório chamado `magic`, como no build do Debian), eles geram um
//! `magic.mgc` idêntico byte a byte ao `/usr/lib/file/magic.mgc` do oráculo.
//!
//! A ordem é a do `strcmp` dos nomes, a mesma em que o `apprentice_load` do libmagic lê o
//! diretório. O nome entra na descrição vazia (`"\0magic/<nome>"`), que pesa no desempate da
//! ordenação.

/// Nome do diretório como o build do Debian passa pro compilador de regras.
pub const DIR_NAME: &str = "magic";

/// `(nome do fragmento, conteúdo)`, na ordem do `strcmp`.
pub const FRAGMENTS: &[(&str, &[u8])] = &[
    (
        "Header",
        include_bytes!("../../vendor/file-5.46/magdir/Header"),
    ),
    (
        "Localstuff",
        include_bytes!("../../vendor/file-5.46/magdir/Localstuff"),
    ),
    (
        "acorn",
        include_bytes!("../../vendor/file-5.46/magdir/acorn"),
    ),
    ("adi", include_bytes!("../../vendor/file-5.46/magdir/adi")),
    (
        "adventure",
        include_bytes!("../../vendor/file-5.46/magdir/adventure"),
    ),
    ("aes", include_bytes!("../../vendor/file-5.46/magdir/aes")),
    (
        "algol68",
        include_bytes!("../../vendor/file-5.46/magdir/algol68"),
    ),
    (
        "allegro",
        include_bytes!("../../vendor/file-5.46/magdir/allegro"),
    ),
    (
        "alliant",
        include_bytes!("../../vendor/file-5.46/magdir/alliant"),
    ),
    (
        "amanda",
        include_bytes!("../../vendor/file-5.46/magdir/amanda"),
    ),
    (
        "amigaos",
        include_bytes!("../../vendor/file-5.46/magdir/amigaos"),
    ),
    (
        "android",
        include_bytes!("../../vendor/file-5.46/magdir/android"),
    ),
    (
        "animation",
        include_bytes!("../../vendor/file-5.46/magdir/animation"),
    ),
    ("aout", include_bytes!("../../vendor/file-5.46/magdir/aout")),
    (
        "apache",
        include_bytes!("../../vendor/file-5.46/magdir/apache"),
    ),
    ("apl", include_bytes!("../../vendor/file-5.46/magdir/apl")),
    (
        "apple",
        include_bytes!("../../vendor/file-5.46/magdir/apple"),
    ),
    (
        "application",
        include_bytes!("../../vendor/file-5.46/magdir/application"),
    ),
    (
        "applix",
        include_bytes!("../../vendor/file-5.46/magdir/applix"),
    ),
    ("apt", include_bytes!("../../vendor/file-5.46/magdir/apt")),
    (
        "archive",
        include_bytes!("../../vendor/file-5.46/magdir/archive"),
    ),
    ("aria", include_bytes!("../../vendor/file-5.46/magdir/aria")),
    ("arm", include_bytes!("../../vendor/file-5.46/magdir/arm")),
    ("asf", include_bytes!("../../vendor/file-5.46/magdir/asf")),
    (
        "assembler",
        include_bytes!("../../vendor/file-5.46/magdir/assembler"),
    ),
    (
        "asterix",
        include_bytes!("../../vendor/file-5.46/magdir/asterix"),
    ),
    (
        "att3b",
        include_bytes!("../../vendor/file-5.46/magdir/att3b"),
    ),
    (
        "audio",
        include_bytes!("../../vendor/file-5.46/magdir/audio"),
    ),
    ("avm", include_bytes!("../../vendor/file-5.46/magdir/avm")),
    (
        "basis",
        include_bytes!("../../vendor/file-5.46/magdir/basis"),
    ),
    (
        "beetle",
        include_bytes!("../../vendor/file-5.46/magdir/beetle"),
    ),
    ("ber", include_bytes!("../../vendor/file-5.46/magdir/ber")),
    ("bflt", include_bytes!("../../vendor/file-5.46/magdir/bflt")),
    ("bhl", include_bytes!("../../vendor/file-5.46/magdir/bhl")),
    (
        "bioinformatics",
        include_bytes!("../../vendor/file-5.46/magdir/bioinformatics"),
    ),
    (
        "biosig",
        include_bytes!("../../vendor/file-5.46/magdir/biosig"),
    ),
    (
        "blackberry",
        include_bytes!("../../vendor/file-5.46/magdir/blackberry"),
    ),
    ("blcr", include_bytes!("../../vendor/file-5.46/magdir/blcr")),
    (
        "blender",
        include_bytes!("../../vendor/file-5.46/magdir/blender"),
    ),
    ("blit", include_bytes!("../../vendor/file-5.46/magdir/blit")),
    ("bm", include_bytes!("../../vendor/file-5.46/magdir/bm")),
    ("bout", include_bytes!("../../vendor/file-5.46/magdir/bout")),
    ("bsdi", include_bytes!("../../vendor/file-5.46/magdir/bsdi")),
    ("bsi", include_bytes!("../../vendor/file-5.46/magdir/bsi")),
    (
        "btsnoop",
        include_bytes!("../../vendor/file-5.46/magdir/btsnoop"),
    ),
    ("burp", include_bytes!("../../vendor/file-5.46/magdir/burp")),
    (
        "bytecode",
        include_bytes!("../../vendor/file-5.46/magdir/bytecode"),
    ),
    (
        "c-lang",
        include_bytes!("../../vendor/file-5.46/magdir/c-lang"),
    ),
    ("c64", include_bytes!("../../vendor/file-5.46/magdir/c64")),
    ("cad", include_bytes!("../../vendor/file-5.46/magdir/cad")),
    (
        "cafebabe",
        include_bytes!("../../vendor/file-5.46/magdir/cafebabe"),
    ),
    ("cbor", include_bytes!("../../vendor/file-5.46/magdir/cbor")),
    ("ccf", include_bytes!("../../vendor/file-5.46/magdir/ccf")),
    ("cddb", include_bytes!("../../vendor/file-5.46/magdir/cddb")),
    (
        "chord",
        include_bytes!("../../vendor/file-5.46/magdir/chord"),
    ),
    (
        "cisco",
        include_bytes!("../../vendor/file-5.46/magdir/cisco"),
    ),
    (
        "citrus",
        include_bytes!("../../vendor/file-5.46/magdir/citrus"),
    ),
    (
        "clarion",
        include_bytes!("../../vendor/file-5.46/magdir/clarion"),
    ),
    (
        "claris",
        include_bytes!("../../vendor/file-5.46/magdir/claris"),
    ),
    (
        "clipper",
        include_bytes!("../../vendor/file-5.46/magdir/clipper"),
    ),
    (
        "clojure",
        include_bytes!("../../vendor/file-5.46/magdir/clojure"),
    ),
    ("coff", include_bytes!("../../vendor/file-5.46/magdir/coff")),
    (
        "commands",
        include_bytes!("../../vendor/file-5.46/magdir/commands"),
    ),
    (
        "communications",
        include_bytes!("../../vendor/file-5.46/magdir/communications"),
    ),
    (
        "compress",
        include_bytes!("../../vendor/file-5.46/magdir/compress"),
    ),
    (
        "console",
        include_bytes!("../../vendor/file-5.46/magdir/console"),
    ),
    (
        "convex",
        include_bytes!("../../vendor/file-5.46/magdir/convex"),
    ),
    (
        "coverage",
        include_bytes!("../../vendor/file-5.46/magdir/coverage"),
    ),
    (
        "cracklib",
        include_bytes!("../../vendor/file-5.46/magdir/cracklib"),
    ),
    (
        "crypto",
        include_bytes!("../../vendor/file-5.46/magdir/crypto"),
    ),
    (
        "ctags",
        include_bytes!("../../vendor/file-5.46/magdir/ctags"),
    ),
    ("ctf", include_bytes!("../../vendor/file-5.46/magdir/ctf")),
    (
        "cubemap",
        include_bytes!("../../vendor/file-5.46/magdir/cubemap"),
    ),
    ("cups", include_bytes!("../../vendor/file-5.46/magdir/cups")),
    ("dact", include_bytes!("../../vendor/file-5.46/magdir/dact")),
    (
        "database",
        include_bytes!("../../vendor/file-5.46/magdir/database"),
    ),
    (
        "dataone",
        include_bytes!("../../vendor/file-5.46/magdir/dataone"),
    ),
    ("dbpf", include_bytes!("../../vendor/file-5.46/magdir/dbpf")),
    (
        "debian-extra-magic",
        include_bytes!("../../vendor/file-5.46/magdir/debian-extra-magic"),
    ),
    ("der", include_bytes!("../../vendor/file-5.46/magdir/der")),
    (
        "diamond",
        include_bytes!("../../vendor/file-5.46/magdir/diamond"),
    ),
    ("dif", include_bytes!("../../vendor/file-5.46/magdir/dif")),
    ("diff", include_bytes!("../../vendor/file-5.46/magdir/diff")),
    (
        "digital",
        include_bytes!("../../vendor/file-5.46/magdir/digital"),
    ),
    (
        "dolby",
        include_bytes!("../../vendor/file-5.46/magdir/dolby"),
    ),
    ("dump", include_bytes!("../../vendor/file-5.46/magdir/dump")),
    (
        "dwarfs",
        include_bytes!("../../vendor/file-5.46/magdir/dwarfs"),
    ),
    (
        "dyadic",
        include_bytes!("../../vendor/file-5.46/magdir/dyadic"),
    ),
    ("ebml", include_bytes!("../../vendor/file-5.46/magdir/ebml")),
    ("edid", include_bytes!("../../vendor/file-5.46/magdir/edid")),
    (
        "editors",
        include_bytes!("../../vendor/file-5.46/magdir/editors"),
    ),
    ("efi", include_bytes!("../../vendor/file-5.46/magdir/efi")),
    ("elf", include_bytes!("../../vendor/file-5.46/magdir/elf")),
    (
        "encore",
        include_bytes!("../../vendor/file-5.46/magdir/encore"),
    ),
    ("epoc", include_bytes!("../../vendor/file-5.46/magdir/epoc")),
    (
        "erlang",
        include_bytes!("../../vendor/file-5.46/magdir/erlang"),
    ),
    (
        "espressif",
        include_bytes!("../../vendor/file-5.46/magdir/espressif"),
    ),
    ("esri", include_bytes!("../../vendor/file-5.46/magdir/esri")),
    ("fcs", include_bytes!("../../vendor/file-5.46/magdir/fcs")),
    (
        "filesystems",
        include_bytes!("../../vendor/file-5.46/magdir/filesystems"),
    ),
    (
        "finger",
        include_bytes!("../../vendor/file-5.46/magdir/finger"),
    ),
    (
        "firmware",
        include_bytes!("../../vendor/file-5.46/magdir/firmware"),
    ),
    (
        "flash",
        include_bytes!("../../vendor/file-5.46/magdir/flash"),
    ),
    ("flif", include_bytes!("../../vendor/file-5.46/magdir/flif")),
    (
        "fonts",
        include_bytes!("../../vendor/file-5.46/magdir/fonts"),
    ),
    (
        "forth",
        include_bytes!("../../vendor/file-5.46/magdir/forth"),
    ),
    (
        "fortran",
        include_bytes!("../../vendor/file-5.46/magdir/fortran"),
    ),
    (
        "frame",
        include_bytes!("../../vendor/file-5.46/magdir/frame"),
    ),
    (
        "freebsd",
        include_bytes!("../../vendor/file-5.46/magdir/freebsd"),
    ),
    ("fsav", include_bytes!("../../vendor/file-5.46/magdir/fsav")),
    (
        "fusecompress",
        include_bytes!("../../vendor/file-5.46/magdir/fusecompress"),
    ),
    (
        "games",
        include_bytes!("../../vendor/file-5.46/magdir/games"),
    ),
    ("gcc", include_bytes!("../../vendor/file-5.46/magdir/gcc")),
    (
        "gconv",
        include_bytes!("../../vendor/file-5.46/magdir/gconv"),
    ),
    (
        "gentoo",
        include_bytes!("../../vendor/file-5.46/magdir/gentoo"),
    ),
    ("geo", include_bytes!("../../vendor/file-5.46/magdir/geo")),
    ("geos", include_bytes!("../../vendor/file-5.46/magdir/geos")),
    ("gimp", include_bytes!("../../vendor/file-5.46/magdir/gimp")),
    ("git", include_bytes!("../../vendor/file-5.46/magdir/git")),
    (
        "glibc",
        include_bytes!("../../vendor/file-5.46/magdir/glibc"),
    ),
    (
        "gnome",
        include_bytes!("../../vendor/file-5.46/magdir/gnome"),
    ),
    ("gnu", include_bytes!("../../vendor/file-5.46/magdir/gnu")),
    (
        "gnumeric",
        include_bytes!("../../vendor/file-5.46/magdir/gnumeric"),
    ),
    ("gpt", include_bytes!("../../vendor/file-5.46/magdir/gpt")),
    ("gpu", include_bytes!("../../vendor/file-5.46/magdir/gpu")),
    (
        "grace",
        include_bytes!("../../vendor/file-5.46/magdir/grace"),
    ),
    (
        "graphviz",
        include_bytes!("../../vendor/file-5.46/magdir/graphviz"),
    ),
    (
        "gringotts",
        include_bytes!("../../vendor/file-5.46/magdir/gringotts"),
    ),
    (
        "hardware",
        include_bytes!("../../vendor/file-5.46/magdir/hardware"),
    ),
    (
        "hitachi-sh",
        include_bytes!("../../vendor/file-5.46/magdir/hitachi-sh"),
    ),
    ("hp", include_bytes!("../../vendor/file-5.46/magdir/hp")),
    (
        "human68k",
        include_bytes!("../../vendor/file-5.46/magdir/human68k"),
    ),
    (
        "ibm370",
        include_bytes!("../../vendor/file-5.46/magdir/ibm370"),
    ),
    (
        "ibm6000",
        include_bytes!("../../vendor/file-5.46/magdir/ibm6000"),
    ),
    ("icc", include_bytes!("../../vendor/file-5.46/magdir/icc")),
    ("iff", include_bytes!("../../vendor/file-5.46/magdir/iff")),
    (
        "images",
        include_bytes!("../../vendor/file-5.46/magdir/images"),
    ),
    (
        "inform",
        include_bytes!("../../vendor/file-5.46/magdir/inform"),
    ),
    (
        "intel",
        include_bytes!("../../vendor/file-5.46/magdir/intel"),
    ),
    (
        "interleaf",
        include_bytes!("../../vendor/file-5.46/magdir/interleaf"),
    ),
    (
        "island",
        include_bytes!("../../vendor/file-5.46/magdir/island"),
    ),
    (
        "ispell",
        include_bytes!("../../vendor/file-5.46/magdir/ispell"),
    ),
    ("isz", include_bytes!("../../vendor/file-5.46/magdir/isz")),
    ("java", include_bytes!("../../vendor/file-5.46/magdir/java")),
    (
        "javascript",
        include_bytes!("../../vendor/file-5.46/magdir/javascript"),
    ),
    ("jpeg", include_bytes!("../../vendor/file-5.46/magdir/jpeg")),
    (
        "karma",
        include_bytes!("../../vendor/file-5.46/magdir/karma"),
    ),
    ("kde", include_bytes!("../../vendor/file-5.46/magdir/kde")),
    (
        "keepass",
        include_bytes!("../../vendor/file-5.46/magdir/keepass"),
    ),
    (
        "kerberos",
        include_bytes!("../../vendor/file-5.46/magdir/kerberos"),
    ),
    (
        "keyman",
        include_bytes!("../../vendor/file-5.46/magdir/keyman"),
    ),
    (
        "kicad",
        include_bytes!("../../vendor/file-5.46/magdir/kicad"),
    ),
    ("kml", include_bytes!("../../vendor/file-5.46/magdir/kml")),
    (
        "lammps",
        include_bytes!("../../vendor/file-5.46/magdir/lammps"),
    ),
    (
        "lauterbach",
        include_bytes!("../../vendor/file-5.46/magdir/lauterbach"),
    ),
    (
        "lecter",
        include_bytes!("../../vendor/file-5.46/magdir/lecter"),
    ),
    ("lex", include_bytes!("../../vendor/file-5.46/magdir/lex")),
    ("lif", include_bytes!("../../vendor/file-5.46/magdir/lif")),
    (
        "linux",
        include_bytes!("../../vendor/file-5.46/magdir/linux"),
    ),
    ("lisp", include_bytes!("../../vendor/file-5.46/magdir/lisp")),
    ("llvm", include_bytes!("../../vendor/file-5.46/magdir/llvm")),
    (
        "locoscript",
        include_bytes!("../../vendor/file-5.46/magdir/locoscript"),
    ),
    ("lua", include_bytes!("../../vendor/file-5.46/magdir/lua")),
    ("luks", include_bytes!("../../vendor/file-5.46/magdir/luks")),
    ("m4", include_bytes!("../../vendor/file-5.46/magdir/m4")),
    ("mach", include_bytes!("../../vendor/file-5.46/magdir/mach")),
    (
        "macintosh",
        include_bytes!("../../vendor/file-5.46/magdir/macintosh"),
    ),
    (
        "macos",
        include_bytes!("../../vendor/file-5.46/magdir/macos"),
    ),
    (
        "magic",
        include_bytes!("../../vendor/file-5.46/magdir/magic"),
    ),
    (
        "mail.news",
        include_bytes!("../../vendor/file-5.46/magdir/mail.news"),
    ),
    ("make", include_bytes!("../../vendor/file-5.46/magdir/make")),
    ("map", include_bytes!("../../vendor/file-5.46/magdir/map")),
    (
        "maple",
        include_bytes!("../../vendor/file-5.46/magdir/maple"),
    ),
    (
        "marc21",
        include_bytes!("../../vendor/file-5.46/magdir/marc21"),
    ),
    (
        "mathcad",
        include_bytes!("../../vendor/file-5.46/magdir/mathcad"),
    ),
    (
        "mathematica",
        include_bytes!("../../vendor/file-5.46/magdir/mathematica"),
    ),
    (
        "matroska",
        include_bytes!("../../vendor/file-5.46/magdir/matroska"),
    ),
    (
        "mcrypt",
        include_bytes!("../../vendor/file-5.46/magdir/mcrypt"),
    ),
    (
        "measure",
        include_bytes!("../../vendor/file-5.46/magdir/measure"),
    ),
    (
        "mercurial",
        include_bytes!("../../vendor/file-5.46/magdir/mercurial"),
    ),
    (
        "metastore",
        include_bytes!("../../vendor/file-5.46/magdir/metastore"),
    ),
    (
        "meteorological",
        include_bytes!("../../vendor/file-5.46/magdir/meteorological"),
    ),
    (
        "microfocus",
        include_bytes!("../../vendor/file-5.46/magdir/microfocus"),
    ),
    ("mime", include_bytes!("../../vendor/file-5.46/magdir/mime")),
    ("mips", include_bytes!("../../vendor/file-5.46/magdir/mips")),
    (
        "mirage",
        include_bytes!("../../vendor/file-5.46/magdir/mirage"),
    ),
    (
        "misctools",
        include_bytes!("../../vendor/file-5.46/magdir/misctools"),
    ),
    ("mkid", include_bytes!("../../vendor/file-5.46/magdir/mkid")),
    (
        "mlssa",
        include_bytes!("../../vendor/file-5.46/magdir/mlssa"),
    ),
    ("mmdf", include_bytes!("../../vendor/file-5.46/magdir/mmdf")),
    (
        "modem",
        include_bytes!("../../vendor/file-5.46/magdir/modem"),
    ),
    (
        "modulefile",
        include_bytes!("../../vendor/file-5.46/magdir/modulefile"),
    ),
    (
        "motorola",
        include_bytes!("../../vendor/file-5.46/magdir/motorola"),
    ),
    (
        "mozilla",
        include_bytes!("../../vendor/file-5.46/magdir/mozilla"),
    ),
    (
        "msdos",
        include_bytes!("../../vendor/file-5.46/magdir/msdos"),
    ),
    (
        "msooxml",
        include_bytes!("../../vendor/file-5.46/magdir/msooxml"),
    ),
    ("msvc", include_bytes!("../../vendor/file-5.46/magdir/msvc")),
    ("msx", include_bytes!("../../vendor/file-5.46/magdir/msx")),
    ("mup", include_bytes!("../../vendor/file-5.46/magdir/mup")),
    (
        "music",
        include_bytes!("../../vendor/file-5.46/magdir/music"),
    ),
    ("nasa", include_bytes!("../../vendor/file-5.46/magdir/nasa")),
    (
        "natinst",
        include_bytes!("../../vendor/file-5.46/magdir/natinst"),
    ),
    ("ncr", include_bytes!("../../vendor/file-5.46/magdir/ncr")),
    (
        "netbsd",
        include_bytes!("../../vendor/file-5.46/magdir/netbsd"),
    ),
    (
        "netscape",
        include_bytes!("../../vendor/file-5.46/magdir/netscape"),
    ),
    (
        "netware",
        include_bytes!("../../vendor/file-5.46/magdir/netware"),
    ),
    ("news", include_bytes!("../../vendor/file-5.46/magdir/news")),
    (
        "nifty",
        include_bytes!("../../vendor/file-5.46/magdir/nifty"),
    ),
    (
        "nim-lang",
        include_bytes!("../../vendor/file-5.46/magdir/nim-lang"),
    ),
    (
        "nitpicker",
        include_bytes!("../../vendor/file-5.46/magdir/nitpicker"),
    ),
    (
        "numpy",
        include_bytes!("../../vendor/file-5.46/magdir/numpy"),
    ),
    (
        "oasis",
        include_bytes!("../../vendor/file-5.46/magdir/oasis"),
    ),
    (
        "ocaml",
        include_bytes!("../../vendor/file-5.46/magdir/ocaml"),
    ),
    (
        "octave",
        include_bytes!("../../vendor/file-5.46/magdir/octave"),
    ),
    (
        "ole2compounddocs",
        include_bytes!("../../vendor/file-5.46/magdir/ole2compounddocs"),
    ),
    ("olf", include_bytes!("../../vendor/file-5.46/magdir/olf")),
    (
        "openfst",
        include_bytes!("../../vendor/file-5.46/magdir/openfst"),
    ),
    (
        "opentimestamps",
        include_bytes!("../../vendor/file-5.46/magdir/opentimestamps"),
    ),
    ("oric", include_bytes!("../../vendor/file-5.46/magdir/oric")),
    ("os2", include_bytes!("../../vendor/file-5.46/magdir/os2")),
    (
        "os400",
        include_bytes!("../../vendor/file-5.46/magdir/os400"),
    ),
    ("os9", include_bytes!("../../vendor/file-5.46/magdir/os9")),
    ("osf1", include_bytes!("../../vendor/file-5.46/magdir/osf1")),
    ("pack", include_bytes!("../../vendor/file-5.46/magdir/pack")),
    ("palm", include_bytes!("../../vendor/file-5.46/magdir/palm")),
    (
        "parix",
        include_bytes!("../../vendor/file-5.46/magdir/parix"),
    ),
    (
        "parrot",
        include_bytes!("../../vendor/file-5.46/magdir/parrot"),
    ),
    (
        "pascal",
        include_bytes!("../../vendor/file-5.46/magdir/pascal"),
    ),
    ("pbf", include_bytes!("../../vendor/file-5.46/magdir/pbf")),
    ("pbm", include_bytes!("../../vendor/file-5.46/magdir/pbm")),
    ("pc98", include_bytes!("../../vendor/file-5.46/magdir/pc98")),
    (
        "pci_ids",
        include_bytes!("../../vendor/file-5.46/magdir/pci_ids"),
    ),
    ("pcjr", include_bytes!("../../vendor/file-5.46/magdir/pcjr")),
    ("pdf", include_bytes!("../../vendor/file-5.46/magdir/pdf")),
    ("pdp", include_bytes!("../../vendor/file-5.46/magdir/pdp")),
    ("perl", include_bytes!("../../vendor/file-5.46/magdir/perl")),
    ("pgf", include_bytes!("../../vendor/file-5.46/magdir/pgf")),
    ("pgp", include_bytes!("../../vendor/file-5.46/magdir/pgp")),
    (
        "pgp-binary-keys",
        include_bytes!("../../vendor/file-5.46/magdir/pgp-binary-keys"),
    ),
    (
        "pkgadd",
        include_bytes!("../../vendor/file-5.46/magdir/pkgadd"),
    ),
    (
        "plan9",
        include_bytes!("../../vendor/file-5.46/magdir/plan9"),
    ),
    (
        "playdate",
        include_bytes!("../../vendor/file-5.46/magdir/playdate"),
    ),
    (
        "plus5",
        include_bytes!("../../vendor/file-5.46/magdir/plus5"),
    ),
    ("pmem", include_bytes!("../../vendor/file-5.46/magdir/pmem")),
    (
        "polyml",
        include_bytes!("../../vendor/file-5.46/magdir/polyml"),
    ),
    (
        "printer",
        include_bytes!("../../vendor/file-5.46/magdir/printer"),
    ),
    (
        "project",
        include_bytes!("../../vendor/file-5.46/magdir/project"),
    ),
    (
        "psdbms",
        include_bytes!("../../vendor/file-5.46/magdir/psdbms"),
    ),
    ("psl", include_bytes!("../../vendor/file-5.46/magdir/psl")),
    (
        "pulsar",
        include_bytes!("../../vendor/file-5.46/magdir/pulsar"),
    ),
    (
        "puzzle",
        include_bytes!("../../vendor/file-5.46/magdir/puzzle"),
    ),
    (
        "pwsafe",
        include_bytes!("../../vendor/file-5.46/magdir/pwsafe"),
    ),
    (
        "pyramid",
        include_bytes!("../../vendor/file-5.46/magdir/pyramid"),
    ),
    (
        "python",
        include_bytes!("../../vendor/file-5.46/magdir/python"),
    ),
    ("qt", include_bytes!("../../vendor/file-5.46/magdir/qt")),
    (
        "revision",
        include_bytes!("../../vendor/file-5.46/magdir/revision"),
    ),
    ("riff", include_bytes!("../../vendor/file-5.46/magdir/riff")),
    (
        "ringdove",
        include_bytes!("../../vendor/file-5.46/magdir/ringdove"),
    ),
    ("rpi", include_bytes!("../../vendor/file-5.46/magdir/rpi")),
    ("rpm", include_bytes!("../../vendor/file-5.46/magdir/rpm")),
    (
        "rpmsg",
        include_bytes!("../../vendor/file-5.46/magdir/rpmsg"),
    ),
    ("rst", include_bytes!("../../vendor/file-5.46/magdir/rst")),
    ("rtf", include_bytes!("../../vendor/file-5.46/magdir/rtf")),
    ("ruby", include_bytes!("../../vendor/file-5.46/magdir/ruby")),
    ("rust", include_bytes!("../../vendor/file-5.46/magdir/rust")),
    ("sc", include_bytes!("../../vendor/file-5.46/magdir/sc")),
    ("sccs", include_bytes!("../../vendor/file-5.46/magdir/sccs")),
    (
        "scientific",
        include_bytes!("../../vendor/file-5.46/magdir/scientific"),
    ),
    (
        "securitycerts",
        include_bytes!("../../vendor/file-5.46/magdir/securitycerts"),
    ),
    (
        "selinux",
        include_bytes!("../../vendor/file-5.46/magdir/selinux"),
    ),
    (
        "sendmail",
        include_bytes!("../../vendor/file-5.46/magdir/sendmail"),
    ),
    (
        "sequent",
        include_bytes!("../../vendor/file-5.46/magdir/sequent"),
    ),
    (
        "sereal",
        include_bytes!("../../vendor/file-5.46/magdir/sereal"),
    ),
    ("sgi", include_bytes!("../../vendor/file-5.46/magdir/sgi")),
    ("sgml", include_bytes!("../../vendor/file-5.46/magdir/sgml")),
    (
        "sharc",
        include_bytes!("../../vendor/file-5.46/magdir/sharc"),
    ),
    (
        "sinclair",
        include_bytes!("../../vendor/file-5.46/magdir/sinclair"),
    ),
    ("sisu", include_bytes!("../../vendor/file-5.46/magdir/sisu")),
    (
        "sketch",
        include_bytes!("../../vendor/file-5.46/magdir/sketch"),
    ),
    (
        "smalltalk",
        include_bytes!("../../vendor/file-5.46/magdir/smalltalk"),
    ),
    (
        "smile",
        include_bytes!("../../vendor/file-5.46/magdir/smile"),
    ),
    (
        "sniffer",
        include_bytes!("../../vendor/file-5.46/magdir/sniffer"),
    ),
    (
        "softquad",
        include_bytes!("../../vendor/file-5.46/magdir/softquad"),
    ),
    ("sosi", include_bytes!("../../vendor/file-5.46/magdir/sosi")),
    ("spec", include_bytes!("../../vendor/file-5.46/magdir/spec")),
    (
        "spectrum",
        include_bytes!("../../vendor/file-5.46/magdir/spectrum"),
    ),
    ("sql", include_bytes!("../../vendor/file-5.46/magdir/sql")),
    ("ssh", include_bytes!("../../vendor/file-5.46/magdir/ssh")),
    ("ssl", include_bytes!("../../vendor/file-5.46/magdir/ssl")),
    (
        "statistics",
        include_bytes!("../../vendor/file-5.46/magdir/statistics"),
    ),
    (
        "subtitle",
        include_bytes!("../../vendor/file-5.46/magdir/subtitle"),
    ),
    ("sun", include_bytes!("../../vendor/file-5.46/magdir/sun")),
    ("svf", include_bytes!("../../vendor/file-5.46/magdir/svf")),
    ("sylk", include_bytes!("../../vendor/file-5.46/magdir/sylk")),
    (
        "symbos",
        include_bytes!("../../vendor/file-5.46/magdir/symbos"),
    ),
    (
        "sysex",
        include_bytes!("../../vendor/file-5.46/magdir/sysex"),
    ),
    ("tcl", include_bytes!("../../vendor/file-5.46/magdir/tcl")),
    (
        "teapot",
        include_bytes!("../../vendor/file-5.46/magdir/teapot"),
    ),
    (
        "terminfo",
        include_bytes!("../../vendor/file-5.46/magdir/terminfo"),
    ),
    ("tex", include_bytes!("../../vendor/file-5.46/magdir/tex")),
    ("tgif", include_bytes!("../../vendor/file-5.46/magdir/tgif")),
    (
        "ti-8x",
        include_bytes!("../../vendor/file-5.46/magdir/ti-8x"),
    ),
    (
        "timezone",
        include_bytes!("../../vendor/file-5.46/magdir/timezone"),
    ),
    (
        "tplink",
        include_bytes!("../../vendor/file-5.46/magdir/tplink"),
    ),
    (
        "troff",
        include_bytes!("../../vendor/file-5.46/magdir/troff"),
    ),
    (
        "tuxedo",
        include_bytes!("../../vendor/file-5.46/magdir/tuxedo"),
    ),
    (
        "typeset",
        include_bytes!("../../vendor/file-5.46/magdir/typeset"),
    ),
    ("uf2", include_bytes!("../../vendor/file-5.46/magdir/uf2")),
    (
        "unicode",
        include_bytes!("../../vendor/file-5.46/magdir/unicode"),
    ),
    (
        "unisig",
        include_bytes!("../../vendor/file-5.46/magdir/unisig"),
    ),
    (
        "unknown",
        include_bytes!("../../vendor/file-5.46/magdir/unknown"),
    ),
    ("usd", include_bytes!("../../vendor/file-5.46/magdir/usd")),
    (
        "uterus",
        include_bytes!("../../vendor/file-5.46/magdir/uterus"),
    ),
    (
        "uuencode",
        include_bytes!("../../vendor/file-5.46/magdir/uuencode"),
    ),
    ("uxn", include_bytes!("../../vendor/file-5.46/magdir/uxn")),
    (
        "vacuum-cleaner",
        include_bytes!("../../vendor/file-5.46/magdir/vacuum-cleaner"),
    ),
    (
        "varied.out",
        include_bytes!("../../vendor/file-5.46/magdir/varied.out"),
    ),
    (
        "varied.script",
        include_bytes!("../../vendor/file-5.46/magdir/varied.script"),
    ),
    ("vax", include_bytes!("../../vendor/file-5.46/magdir/vax")),
    (
        "vicar",
        include_bytes!("../../vendor/file-5.46/magdir/vicar"),
    ),
    (
        "virtual",
        include_bytes!("../../vendor/file-5.46/magdir/virtual"),
    ),
    (
        "virtutech",
        include_bytes!("../../vendor/file-5.46/magdir/virtutech"),
    ),
    ("visx", include_bytes!("../../vendor/file-5.46/magdir/visx")),
    ("vms", include_bytes!("../../vendor/file-5.46/magdir/vms")),
    (
        "vmware",
        include_bytes!("../../vendor/file-5.46/magdir/vmware"),
    ),
    (
        "vorbis",
        include_bytes!("../../vendor/file-5.46/magdir/vorbis"),
    ),
    ("vxl", include_bytes!("../../vendor/file-5.46/magdir/vxl")),
    ("warc", include_bytes!("../../vendor/file-5.46/magdir/warc")),
    ("weak", include_bytes!("../../vendor/file-5.46/magdir/weak")),
    ("web", include_bytes!("../../vendor/file-5.46/magdir/web")),
    (
        "webassembly",
        include_bytes!("../../vendor/file-5.46/magdir/webassembly"),
    ),
    (
        "windows",
        include_bytes!("../../vendor/file-5.46/magdir/windows"),
    ),
    (
        "wireless",
        include_bytes!("../../vendor/file-5.46/magdir/wireless"),
    ),
    (
        "wordprocessors",
        include_bytes!("../../vendor/file-5.46/magdir/wordprocessors"),
    ),
    ("wsdl", include_bytes!("../../vendor/file-5.46/magdir/wsdl")),
    (
        "x68000",
        include_bytes!("../../vendor/file-5.46/magdir/x68000"),
    ),
    (
        "xdelta",
        include_bytes!("../../vendor/file-5.46/magdir/xdelta"),
    ),
    (
        "xenix",
        include_bytes!("../../vendor/file-5.46/magdir/xenix"),
    ),
    (
        "xilinx",
        include_bytes!("../../vendor/file-5.46/magdir/xilinx"),
    ),
    ("xo65", include_bytes!("../../vendor/file-5.46/magdir/xo65")),
    (
        "xwindows",
        include_bytes!("../../vendor/file-5.46/magdir/xwindows"),
    ),
    ("yara", include_bytes!("../../vendor/file-5.46/magdir/yara")),
    ("zfs", include_bytes!("../../vendor/file-5.46/magdir/zfs")),
    (
        "zilog",
        include_bytes!("../../vendor/file-5.46/magdir/zilog"),
    ),
    ("zip", include_bytes!("../../vendor/file-5.46/magdir/zip")),
    (
        "zyxel",
        include_bytes!("../../vendor/file-5.46/magdir/zyxel"),
    ),
];
