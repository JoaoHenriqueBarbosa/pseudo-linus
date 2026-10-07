//! Testes do hexdump no kernel de teste. As saídas esperadas foram capturadas do hexdump do
//! util-linux 2.41.5 no oráculo (`pseudo-linus-oracle:719900900623`, Debian 13).

use sysabi::testkit::TestKit;

fn kit() -> TestKit {
    TestKit::new()
        .programs(crate::programs())
        .dir("/work", 0o755)
        .cwd("/work")
}

fn run(k: &TestKit, argv: &[&str], stdin: &[u8]) -> (String, String, i32) {
    let r = k.run(argv, stdin);
    (r.stdout_str(), r.stderr_str(), r.status.shell_status())
}

const DIGITS: &[u8] = b"0123456789abcdefghij";

#[test]
fn default_and_canonical() {
    let k = kit().file("/work/a", DIGITS, 0o644);
    let (out, err, code) = run(&k, &["hexdump", "a"], b"");
    assert_eq!(
        out,
        "0000000 3130 3332 3534 3736 3938 6261 6463 6665\n0000010 6867 6a69                              \n0000014\n"
    );
    assert_eq!((err.as_str(), code), ("", 0));
    let (out, _, _) = run(&k, &["hexdump", "-C", "a"], b"");
    assert_eq!(
        out,
        "00000000  30 31 32 33 34 35 36 37  38 39 61 62 63 64 65 66  |0123456789abcdef|\n\
         00000010  67 68 69 6a                                       |ghij|\n00000014\n"
    );
    let (hd, _, _) = run(&k, &["hd", "a"], b"");
    assert_eq!(hd, out);
}

#[test]
fn one_byte_formats() {
    let k = kit().file("/work/a", DIGITS, 0o644);
    let (out, _, _) = run(&k, &["hexdump", "-b", "a"], b"");
    assert_eq!(
        out,
        "0000000 060 061 062 063 064 065 066 067 070 071 141 142 143 144 145 146\n\
         0000010 147 150 151 152                                                \n0000014\n"
    );
    let (out, _, _) = run(&k, &["hexdump", "-X", "a"], b"");
    assert_eq!(
        out,
        "0000000  30  31  32  33  34  35  36  37  38  39  61  62  63  64  65  66\n\
         0000010  67  68  69  6a                                                \n0000014\n"
    );
    let (out, _, _) = run(&k, &["hexdump", "-d", "a"], b"");
    assert_eq!(
        out,
        "0000000   12592   13106   13620   14134   14648   25185   25699   26213\n\
         0000010   26727   27241                                                \n0000014\n"
    );
    let (out, _, _) = run(&k, &["hexdump", "-o", "a"], b"");
    assert_eq!(
        out,
        "0000000  030460  031462  032464  033466  034470  061141  062143  063145\n\
         0000010  064147  065151                                                \n0000014\n"
    );
}

#[test]
fn char_formats() {
    let k = kit();
    let (out, _, _) = run(
        &k,
        &["hexdump", "-c"],
        b"\x00\x01\x7f\x80\xff\n\t\r\x07\x08\x0c\x0b ~\xc3\xa9",
    );
    assert_eq!(
        out,
        "0000000  \\0 001 177 200 377  \\n  \\t  \\r  \\a  \\b  \\f  \\v       ~ 303 251\n0000010\n"
    );
    let (out, _, _) = run(
        &k,
        &["hexdump", "-e", "16/1 \"%_u \" \"\\n\""],
        b"\x00\x01\x7f\x80\xff\n\t\r\x07\x08\x0c\x0b ~",
    );
    assert_eq!(out, "nul soh del 80 ff lf ht cr bel bs ff vt   ~  \n");
}

#[test]
fn squeeze_and_v() {
    let k = kit();
    let zeros = [0u8; 64];
    let (out, _, _) = run(&k, &["hexdump"], &zeros);
    assert_eq!(
        out,
        "0000000 0000 0000 0000 0000 0000 0000 0000 0000\n*\n0000040\n"
    );
    let mut data = vec![0u8; 32];
    data.extend_from_slice(b"abc");
    data.extend_from_slice(&[0u8; 45]);
    let (out, _, _) = run(&k, &["hexdump", "-C"], &data);
    assert_eq!(
        out,
        "00000000  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|\n*\n\
         00000020  61 62 63 00 00 00 00 00  00 00 00 00 00 00 00 00  |abc.............|\n\
         00000030  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|\n*\n00000050\n"
    );
    let (out, _, _) = run(&k, &["hexdump", "-v", "-C"], &zeros[..32]);
    assert_eq!(
        out,
        "00000000  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|\n\
         00000010  00 00 00 00 00 00 00 00  00 00 00 00 00 00 00 00  |................|\n00000020\n"
    );
}

#[test]
fn errors_and_files() {
    let k = kit().file("/work/a", DIGITS, 0o644).dir("/work/d", 0o755);
    let (out, err, code) = run(&k, &["hexdump", "nonexist"], b"");
    assert_eq!(out, "");
    assert_eq!(
        err,
        "hexdump: nonexist: No such file or directory\nhexdump: all input file arguments failed\n"
    );
    assert_eq!(code, 1);
    let (_, err, code) = run(&k, &["hexdump", "d"], b"");
    assert_eq!((err.as_str(), code), ("hexdump: d: Is a directory\n", 0));
    let (out, err, code) = run(&k, &["hexdump", "-s", "2"], b"hello\n");
    assert_eq!(
        (out.as_str(), err.as_str(), code),
        ("", "hexdump: stdin: Illegal seek\n", 1)
    );
    let (out, _, _) = run(&k, &["hexdump", "-s", "30", "-C", "a", "a"], b"");
    assert_eq!(
        out,
        "0000001e  61 62 63 64 65 66 67 68  69 6a                    |abcdefghij|\n00000028\n"
    );
    let (out, _, _) = run(&k, &["hexdump", "-s", "21", "a"], b"");
    assert_eq!(out, "0000014\n");
    let (_, err, code) = run(&k, &["hexdump", "-n", "abc", "a"], b"");
    assert_eq!(
        (err.as_str(), code),
        (
            "hexdump: failed to parse length: 'abc': Invalid argument\n",
            1
        )
    );
    let (_, err, code) = run(&k, &["hexdump", "-Q"], b"");
    assert_eq!(
        err,
        "hexdump: invalid option -- 'Q'\nTry 'hexdump --help' for more information.\n"
    );
    assert_eq!(code, 1);
    let (out, _, code) = run(&k, &["hd", "-V"], b"");
    assert_eq!((out.as_str(), code), ("hd from util-linux 2.41.5\n", 0));
}

#[test]
fn format_errors() {
    let k = kit().file("/work/b", b"\xff\xfe\x01\x80abcdefgh12345678", 0o644);
    let cases: &[(&str, &str)] = &[
        ("\"%%\"", "hexdump: bad conversion character %%\n"),
        ("\"%\"", "hexdump: bad format {%}\n"),
        (
            "1/3 \"%d\"",
            "hexdump: bad byte count for conversion character d\n",
        ),
        (
            "1/16 \"%f\\n\"",
            "hexdump: bad byte count for conversion character f\n",
        ),
        ("4/1", "hexdump: bad format {4/1}\n"),
        ("4\"%x \"", "hexdump: bad format {4\"%x \"}\n"),
        (
            "\"%s\"",
            "hexdump: %s requires a precision or a byte count\n",
        ),
        ("\"%_Z\"", "hexdump: bad conversion character %_Z\n"),
        ("\"%_ap\"", "hexdump: bad conversion character %_ap\n"),
        (
            "1/1 \"%x %x\"",
            "hexdump: byte count with multiple conversion characters\n",
        ),
        (
            "1/2 \"%_c\"",
            "hexdump: bad byte count for conversion character _c\n",
        ),
        (
            "4/1 \"%02x_L[red \" \"\\n\"",
            "hexdump: bad conversion character %_L\n",
        ),
    ];
    for (fmt, msg) in cases {
        let (out, err, code) = run(&k, &["hexdump", "-e", fmt, "b"], b"");
        assert_eq!(
            (out.as_str(), err.as_str(), code),
            ("", *msg, 1),
            "formato {fmt}"
        );
    }
}

#[test]
fn custom_formats() {
    let k = kit()
        .file("/work/b", b"\xff\xfe\x01\x80abcdefgh12345678", 0o644)
        .file("/work/a", DIGITS, 0o644);
    let (out, _, _) = run(&k, &["hexdump", "-e", "1/1 \"%d \"", "b"], b"");
    assert_eq!(
        out,
        "-1 -2 1 -128 97 98 99 100 101 102 103 104 49 50 51 52 53 54 55 56 "
    );
    let (out, _, _) = run(&k, &["hexdump", "-e", "2/1 \"%d \"", "b"], b"");
    assert_eq!(
        out,
        "-1 -21 -12897 9899 100101 102103 10449 5051 5253 5455 56"
    );
    let r = k.run(&["hexdump", "-e", "\"%5.3s|\"", "b"], b"");
    assert_eq!(
        r.stdout,
        b"  \xff\xfe\x01|  \x80ab|  cde|  fgh|  123|  456|   78|"
    );
    let (out, _, _) = run(
        &k,
        &["hexdump", "-e", "\"[%6_ad]\" 4/1 \"%x\" \"\\n\"", "b"],
        b"",
    );
    assert_eq!(
        out,
        "[     0]fffe180\n[     4]61626364\n[     8]65666768\n[    12]31323334\n[    16]35363738\n"
    );
    let (out, _, _) = run(
        &k,
        &[
            "hexdump",
            "-e",
            "\"%_Ad|%_ax\\n\"",
            "-e",
            "4/1 \"%02x\" \"\\n\"",
            "a",
        ],
        b"",
    );
    assert_eq!(
        out,
        "30313233\n34353637\n38396162\n63646566\n6768696a\n20|14\n"
    );
    let (out, _, _) = run(&k, &["hexdump", "-e", "2/1 \"%x\\n\"", "a"], b"");
    assert_eq!(
        out,
        "30\n3132\n3334\n3536\n3738\n3961\n6263\n6465\n6667\n6869\n6a"
    );
    let (out, _, _) = run(&k, &["hexdump", "-e", "2/3 \"%s|\" \"\\n\"", "a"], b"");
    assert_eq!(out, "012345|345|\n6789ab|9ab|\ncdefgh|fgh|\nij||\n");
}

#[test]
fn colors_always() {
    let k = kit().file("/work/v", b"ABCDABCD", 0o644);
    let (out, _, _) = run(
        &k,
        &[
            "hexdump",
            "--color=always",
            "-e",
            "8/1 \"%02x_L[red:0x41,blue:0x42,green] \" \"\\n\"",
            "v",
        ],
        b"",
    );
    assert_eq!(
        out,
        "\x1b[31m41\x1b[0m \x1b[34m42\x1b[0m \x1b[32m43\x1b[0m \x1b[32m44\x1b[0m \
         \x1b[31m41\x1b[0m \x1b[34m42\x1b[0m \x1b[32m43\x1b[0m \x1b[32m44\x1b[0m\n"
    );
    let (out, _, _) = run(
        &k,
        &["hexdump", "-e", "4/1 \"%02x_L[red] \" \"\\n\"", "v"],
        b"",
    );
    assert_eq!(out, "41 42 43 44\n*\n");
    let (out, _, _) = run(
        &k,
        &[
            "hexdump",
            "--color=always",
            "-e",
            "8/1 \"%02x_L[red@2-4] \" \"\\n\"",
            "v",
        ],
        b"",
    );
    assert_eq!(
        out,
        "41 42 \x1b[31m43\x1b[0m \x1b[31m44\x1b[0m \x1b[31m41\x1b[0m 42 43 44\n"
    );
    let (_, err, code) = run(&k, &["hexdump", "--color=foo", "v"], b"");
    assert_eq!(
        (err.as_str(), code),
        ("hexdump: unsupported color mode: 'foo'\n", 1)
    );
}
