//! Tests for checksum-based fingerprinting (rebuild detection).

use std::fs::{self, OpenOptions};
use std::io::prelude::*;
use std::net::TcpListener;
use std::process::Stdio;
use std::thread;

use cargo_test_support::assert_deps_contains;
use cargo_test_support::prelude::*;
use cargo_test_support::registry::Package;
use cargo_test_support::{
    basic_lib_manifest, basic_manifest, project, rustc_host, rustc_host_env, str,
};

use super::death;

#[cargo_test]
fn non_nightly_fails() {
    let p = project().file("src/main.rs", "fn main() {}").build();
    p.cargo("build -Zchecksum-freshness")
        .with_stderr_data(str![[r#"
[ERROR] the `-Z` flag is only accepted on the nightly channel of Cargo, but this is the `stable` channel
See https://doc.rust-lang.org/book/appendix-07-nightly-rust.html for more information about Rust release channels.

"#]])
        .with_status(101)
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn checksum_actually_uses_checksum() {
    let p = project()
        .file("src/main.rs", "mod a; fn main() {}")
        .file("src/a.rs", "")
        .build();

    p.cargo("check -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[CHECKING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.root().move_into_the_future();

    p.cargo("check -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn checksum_build_compatible_with_mtime_build() {
    let p = project()
        .file("src/main.rs", "mod a; fn main() {}")
        .file("src/a.rs", "")
        .build();

    p.cargo("check -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[CHECKING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("check")
        .with_stderr_data(str![[r#"
[CHECKING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("check -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[CHECKING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("check")
        .with_stderr_data(str![[r#"
[CHECKING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn same_size_different_content() {
    let p = project()
        .file("src/main.rs", "mod a; fn main() {}")
        .file("src/a.rs", "")
        .build();

    p.cargo("check -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[CHECKING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.change_file("src/main.rs", "mod a;fn main() { }");

    p.cargo("check -v -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the file `src/main.rs` has changed (checksum didn't match, blake3=26aa07e1adab787246f9d333be65d2eb78dd5fd0fee834ba7a769098b4b651bc != blake3=fc1a42e376d9c148227c13de41b77143f6b5b8132d2b204b63cdbc9326848894)
[CHECKING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("check -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(
    nightly,
    reason = "-Zbinary-dep-depinfo is unstable, also requires -Zchecksum-hash-algorithm"
)]
fn binary_depinfo_correctly_encoded() {
    Package::new("regdep", "0.1.0")
        .file("src/lib.rs", "pub fn f() {}")
        .publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.1.0"
            edition = "2018"

            [dependencies]
            regdep = "0.1"
            bar = {path = "./bar"}
            "#,
        )
        .file(
            "src/main.rs",
            r#"
            fn main() {
                regdep::f();
                bar::f();
            }
            "#,
        )
        /*********** Path Dependency `bar` ***********/
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.1.0"))
        .file("bar/src/lib.rs", "pub fn f() {}")
        .build();

    let host = rustc_host();
    p.cargo("build -Zbinary-dep-depinfo -Zchecksum-freshness --target")
        .arg(&host)
        .masquerade_as_nightly_cargo(&["binary-dep-depinfo", "checksum-freshness"])
        .with_stderr_data(str![[r#"
...
[COMPILING] foo v0.1.0 ([ROOT]/foo)
...

"#]])
        .run();

    assert_deps_contains(
        &p,
        &format!("target/{}/debug/.fingerprint/foo-*/dep-bin-foo", host),
        &[
            (0, "src/main.rs"),
            (1, &format!("{}/debug/deps/libbar-*.rlib", host)),
            (1, &format!("{}/debug/deps/libregdep-*.rlib", host)),
        ],
    );

    // Make sure it stays fresh.
    p.cargo("build -Zbinary-dep-depinfo -Zchecksum-freshness --target")
        .arg(&host)
        .masquerade_as_nightly_cargo(&["binary-dep-depinfo", "checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn modifying_and_moving() {
    let p = project()
        .file("src/main.rs", "mod a; fn main() {}")
        .file("src/a.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.root().move_into_the_past();
    p.root().join("target").move_into_the_past();

    p.change_file("src/a.rs", "#[allow(unused)]fn main() {}");
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): file size changed (0 != 28) for `src/a.rs`
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    fs::rename(&p.root().join("src/a.rs"), &p.root().join("src/b.rs")).unwrap();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_status(101)
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
error[E0583]: file not found for module `a`
...
[ERROR] could not compile `foo` (bin "foo") due to 1 previous error

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn rebuild_sub_package_then_while_package() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                authors = []
                version = "0.0.1"
                edition = "2015"

                [dependencies.a]
                path = "a"
                [dependencies.b]
                path = "b"
            "#,
        )
        .file("src/lib.rs", "extern crate a; extern crate b;")
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                authors = []
                version = "0.0.1"
                edition = "2015"
                [dependencies.b]
                path = "../b"
            "#,
        )
        .file("a/src/lib.rs", "extern crate b;")
        .file("b/Cargo.toml", &basic_manifest("b", "0.0.1"))
        .file("b/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[LOCKING] 2 packages to latest compatible versions
[COMPILING] b v0.0.1 ([ROOT]/foo/b)
[COMPILING] a v0.0.1 ([ROOT]/foo/a)
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.change_file("b/src/lib.rs", "pub fn b() {}");

    p.cargo("build -Zchecksum-freshness -pb -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[DIRTY] b v0.0.1 ([ROOT]/foo/b): file size changed (0 != 13) for `b/src/lib.rs`
[COMPILING] b v0.0.1 ([ROOT]/foo/b)
[RUNNING] `rustc --crate-name b [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.change_file(
        "src/lib.rs",
        "extern crate a; extern crate b; pub fn toplevel() {}",
    );

    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FRESH] b v0.0.1 ([ROOT]/foo/b)
[DIRTY] a v0.0.1 ([ROOT]/foo/a): the dependency b was rebuilt ([TIME_DIFF_AFTER_LAST_BUILD])
[COMPILING] a v0.0.1 ([ROOT]/foo/a)
[RUNNING] `rustc --crate-name a [..]
[DIRTY] foo v0.0.1 ([ROOT]/foo): the dependency b was rebuilt ([TIME_DIFF_AFTER_LAST_BUILD])
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo [..] src/lib.rs [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn changing_lib_features_caches_targets() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                authors = []
                version = "0.0.1"
                edition = "2015"

                [features]
                foo = []
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness --features foo")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    /* Targets should be cached from the first build */

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness --features foo")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn changing_profiles_caches_targets() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                authors = []
                version = "0.0.1"
                edition = "2015"

                [profile.dev]
                panic = "abort"
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("test -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `test` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] unittests src/lib.rs (target/debug/deps/foo-[HASH][EXE])
[DOCTEST] foo

"#]])
        .run();

    /* Targets should be cached from the first build */

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("test -Zchecksum-freshness foo")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `test` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] unittests src/lib.rs (target/debug/deps/foo-[HASH][EXE])

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn changing_bin_paths_common_target_features_caches_targets() {
    // Make sure dep_cache crate is built once per feature
    let p = project()
        .no_manifest()
        .file(
            ".cargo/config.toml",
            r#"
                [build]
                target-dir = "./target"
            "#,
        )
        .file(
            "dep_crate/Cargo.toml",
            r#"
                [package]
                name    = "dep_crate"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [features]
                ftest  = []
            "#,
        )
        .file(
            "dep_crate/src/lib.rs",
            r#"
                #[cfg(feature = "ftest")]
                pub fn yo() {
                    println!("ftest on")
                }
                #[cfg(not(feature = "ftest"))]
                pub fn yo() {
                    println!("ftest off")
                }
            "#,
        )
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name    = "a"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [dependencies]
                dep_crate = {path = "../dep_crate", features = []}
            "#,
        )
        .file("a/src/lib.rs", "")
        .file(
            "a/src/main.rs",
            r#"
                extern crate dep_crate;
                use dep_crate::yo;
                fn main() {
                    yo();
                }
            "#,
        )
        .file(
            "b/Cargo.toml",
            r#"
                [package]
                name    = "b"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [dependencies]
                dep_crate = {path = "../dep_crate", features = ["ftest"]}
            "#,
        )
        .file("b/src/lib.rs", "")
        .file(
            "b/src/main.rs",
            r#"
                extern crate dep_crate;
                use dep_crate::yo;
                fn main() {
                    yo();
                }
            "#,
        )
        .build();

    /* Build and rebuild a/. Ensure dep_crate only builds once */
    p.cargo("run -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd("a")
        .with_stdout_data(str![[r#"
ftest off

"#]])
        .with_stderr_data(str![[r#"
[LOCKING] 1 package to latest compatible version
[COMPILING] dep_crate v0.0.1 ([ROOT]/foo/dep_crate)
[COMPILING] a v0.0.1 ([ROOT]/foo/a)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/foo/./target/debug/a[EXE]`

"#]])
        .run();
    p.cargo("clean -p a").cwd("a").run();
    p.cargo("run -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd("a")
        .with_stdout_data(str![[r#"
ftest off

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] a v0.0.1 ([ROOT]/foo/a)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/foo/./target/debug/a[EXE]`

"#]])
        .run();

    /* Build and rebuild b/. Ensure dep_crate only builds once */
    p.cargo("run -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd("b")
        .with_stdout_data(str![[r#"
ftest on

"#]])
        .with_stderr_data(str![[r#"
[LOCKING] 1 package to latest compatible version
[COMPILING] dep_crate v0.0.1 ([ROOT]/foo/dep_crate)
[COMPILING] b v0.0.1 ([ROOT]/foo/b)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/foo/./target/debug/b[EXE]`

"#]])
        .run();
    p.cargo("clean -p b").cwd("b").run();
    p.cargo("run -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd("b")
        .with_stdout_data(str![[r#"
ftest on

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] b v0.0.1 ([ROOT]/foo/b)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/foo/./target/debug/b[EXE]`

"#]])
        .run();

    /* Build a/ package again. If we cache different feature dep builds correctly,
     * this should not cause a rebuild of dep_crate */
    p.cargo("clean -p a").cwd("a").run();
    p.cargo("run -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd("a")
        .with_stdout_data(str![[r#"
ftest off

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] a v0.0.1 ([ROOT]/foo/a)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/foo/./target/debug/a[EXE]`

"#]])
        .run();

    /* Build b/ package again. If we cache different feature dep builds correctly,
     * this should not cause a rebuild */
    p.cargo("clean -p b").cwd("b").run();
    p.cargo("run -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd("b")
        .with_stdout_data(str![[r#"
ftest on

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] b v0.0.1 ([ROOT]/foo/b)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/foo/./target/debug/b[EXE]`

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn changing_bin_features_caches_targets() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                authors = []
                version = "0.0.1"
                edition = "2015"

                [features]
                foo = []
            "#,
        )
        .file(
            "src/main.rs",
            r#"
                fn main() {
                    let msg = if cfg!(feature = "foo") { "feature on" } else { "feature off" };
                    println!("{}", msg);
                }
            "#,
        )
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.rename_run("foo", "off1")
        .with_stdout_data(str![[r#"
feature off

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness --features foo")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.rename_run("foo", "on1")
        .with_stdout_data(str![[r#"
feature on

"#]])
        .run();

    /* Targets should be cached from the first build */

    let mut e = p.cargo("build -Zchecksum-freshness -v");
    e.masquerade_as_nightly_cargo(&["checksum-freshness"]);

    // MSVC does not include hash in binary filename, so it gets recompiled.
    if cfg!(target_env = "msvc") {
        e.with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the list of features changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]]);
    } else {
        e.with_stderr_data(str![[r#"
[FRESH] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]]);
    }
    e.run();
    p.rename_run("foo", "off2")
        .with_stdout_data(str![[r#"
feature off

"#]])
        .run();

    let mut e = p.cargo("build -Zchecksum-freshness --features foo -v");
    e.masquerade_as_nightly_cargo(&["checksum-freshness"]);
    if cfg!(target_env = "msvc") {
        e.with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the list of features changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]]);
    } else {
        e.with_stderr_data(str![[r#"
[FRESH] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]]);
    }
    e.run();
    p.rename_run("foo", "on2")
        .with_stdout_data(str![[r#"
feature on

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn rebuild_tests_if_lib_changes() {
    let p = project()
        .file("src/lib.rs", "pub fn foo() {}")
        .file("tests/foo-test.rs", "extern crate foo;")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("test -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    p.change_file("src/lib.rs", "");

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("test -Zchecksum-freshness -v --test foo-test")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the dependency foo was rebuilt ([TIME_DIFF_AFTER_LAST_BUILD])
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo_test [..]`
[FINISHED] `test` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/foo/target/debug/deps/foo_test-[HASH][EXE]`

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn no_rebuild_transitive_target_deps() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [dependencies]
                a = { path = "a" }
                [dev-dependencies]
                b = { path = "b" }
            "#,
        )
        .file("src/lib.rs", "")
        .file("tests/foo.rs", "")
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [target.foo.dependencies]
                c = { path = "../c" }
            "#,
        )
        .file("a/src/lib.rs", "")
        .file(
            "b/Cargo.toml",
            r#"
                [package]
                name = "b"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [dependencies]
                c = { path = "../c" }
            "#,
        )
        .file("b/src/lib.rs", "")
        .file("c/Cargo.toml", &basic_manifest("c", "0.0.1"))
        .file("c/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("test -Zchecksum-freshness --no-run")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] c v0.0.1 ([ROOT]/foo/c)
[COMPILING] b v0.0.1 ([ROOT]/foo/b)
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `test` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[EXECUTABLE] unittests src/lib.rs (target/debug/deps/foo-[HASH][EXE])
[EXECUTABLE] tests/foo.rs (target/debug/deps/foo-[HASH][EXE])

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn rerun_if_changed_in_dep() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [dependencies]
                a = { path = "a" }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.0.1"
                edition = "2015"
                authors = []
                build = "build.rs"
            "#,
        )
        .file(
            "a/build.rs",
            r#"
                fn main() {
                    println!("cargo::rerun-if-changed=build.rs");
                }
            "#,
        )
        .file("a/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn same_build_dir_cached_packages() {
    let p = project()
        .no_manifest()
        .file(
            "a1/Cargo.toml",
            r#"
                [package]
                name = "a1"
                version = "0.0.1"
                edition = "2015"
                authors = []
                [dependencies]
                b = { path = "../b" }
            "#,
        )
        .file("a1/src/lib.rs", "")
        .file(
            "a2/Cargo.toml",
            r#"
                [package]
                name = "a2"
                version = "0.0.1"
                edition = "2015"
                authors = []
                [dependencies]
                b = { path = "../b" }
            "#,
        )
        .file("a2/src/lib.rs", "")
        .file(
            "b/Cargo.toml",
            r#"
                [package]
                name = "b"
                version = "0.0.1"
                edition = "2015"
                authors = []
                [dependencies]
                c = { path = "../c" }
            "#,
        )
        .file("b/src/lib.rs", "")
        .file(
            "c/Cargo.toml",
            r#"
                [package]
                name = "c"
                version = "0.0.1"
                edition = "2015"
                authors = []
                [dependencies]
                d = { path = "../d" }
            "#,
        )
        .file("c/src/lib.rs", "")
        .file("d/Cargo.toml", &basic_manifest("d", "0.0.1"))
        .file("d/src/lib.rs", "")
        .file(
            ".cargo/config.toml",
            r#"
                [build]
                target-dir = "./target"
            "#,
        )
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd("a1")
        .with_stderr_data(str![[r#"
[LOCKING] 3 packages to latest compatible versions
[COMPILING] d v0.0.1 ([ROOT]/foo/d)
[COMPILING] c v0.0.1 ([ROOT]/foo/c)
[COMPILING] b v0.0.1 ([ROOT]/foo/b)
[COMPILING] a1 v0.0.1 ([ROOT]/foo/a1)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd("a2")
        .with_stderr_data(str![[r#"
[LOCKING] 3 packages to latest compatible versions
[COMPILING] a2 v0.0.1 ([ROOT]/foo/a2)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn no_rebuild_if_build_artifacts_move_backwards_in_time() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [dependencies]
                a = { path = "a" }
            "#,
        )
        .file("src/lib.rs", "")
        .file("a/Cargo.toml", &basic_manifest("a", "0.0.1"))
        .file("a/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    p.root().move_into_the_past();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stdout_data(str![])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn rebuild_if_environment_changes() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                description = "old desc"
                version = "0.0.1"
                edition = "2015"
                authors = []
            "#,
        )
        .file(
            "src/main.rs",
            r#"
                fn main() {
                    println!("{}", env!("CARGO_PKG_DESCRIPTION"));
                }
            "#,
        )
        .build();

    p.cargo("run -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stdout_data(str![[r#"
old desc

"#]])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `target/debug/foo[EXE]`

"#]])
        .run();

    p.change_file(
        "Cargo.toml",
        r#"
            [package]
            name = "foo"
            description = "new desc"
            version = "0.0.1"
            edition = "2015"
            authors = []
        "#,
    );

    p.cargo("run -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stdout_data(str![[r#"
new desc

"#]])
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the metadata changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `target/debug/foo[EXE]`

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn no_rebuild_when_rename_dir() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                edition = "2015"
                authors = []

                [workspace]

                [dependencies]
                foo = { path = "foo" }
            "#,
        )
        .file("src/_unused.rs", "")
        .file("build.rs", "fn main() {}")
        .file("foo/Cargo.toml", &basic_manifest("foo", "0.0.1"))
        .file("foo/src/lib.rs", "")
        .file("foo/build.rs", "fn main() {}")
        .build();

    // make sure the most recently modified file is `src/lib.rs`, not
    // `Cargo.toml`, to expose a historical bug where we forgot to strip the
    // `Cargo.toml` path from looking for the package root.
    fs::write(p.root().join("src/lib.rs"), "").unwrap();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    let mut new = p.root();
    new.pop();
    new.push("bar");
    fs::rename(p.root(), &new).unwrap();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd(&new)
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn unused_optional_dep() {
    Package::new("registry1", "0.1.0").publish();
    Package::new("registry2", "0.1.0").publish();
    Package::new("registry3", "0.1.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "p"
                authors = []
                version = "0.1.0"
                edition = "2015"

                [dependencies]
                bar = { path = "bar" }
                baz = { path = "baz" }
                registry1 = "*"
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "bar/Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.1.1"
                edition = "2015"
                authors = []

                [dev-dependencies]
                registry2 = "*"
            "#,
        )
        .file("bar/src/lib.rs", "")
        .file(
            "baz/Cargo.toml",
            r#"
                [package]
                name = "baz"
                version = "0.1.1"
                edition = "2015"
                authors = []

                [dependencies]
                registry3 = { version = "*", optional = true }
            "#,
        )
        .file("baz/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn path_dev_dep_registry_updates() {
    Package::new("registry1", "0.1.0").publish();
    Package::new("registry2", "0.1.0").publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "p"
                authors = []
                version = "0.1.0"
                edition = "2015"

                [dependencies]
                bar = { path = "bar" }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "bar/Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.1.1"
                edition = "2015"
                authors = []

                [dependencies]
                registry1 = "*"

                [dev-dependencies]
                baz = { path = "../baz"}
            "#,
        )
        .file("bar/src/lib.rs", "")
        .file(
            "baz/Cargo.toml",
            r#"
                [package]
                name = "baz"
                version = "0.1.1"
                edition = "2015"
                authors = []

                [dependencies]
                registry2 = "*"
            "#,
        )
        .file("baz/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn change_panic_mode() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [workspace]
                members = ['bar', 'baz']
                [profile.dev]
                panic = 'abort'
            "#,
        )
        .file("src/lib.rs", "")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.1.1"))
        .file("bar/src/lib.rs", "")
        .file(
            "baz/Cargo.toml",
            r#"
                [package]
                name = "baz"
                version = "0.1.1"
                edition = "2015"
                authors = []

                [lib]
                proc-macro = true

                [dependencies]
                bar = { path = '../bar' }
            "#,
        )
        .file("baz/src/lib.rs", "extern crate bar;")
        .build();

    p.cargo("build -Zchecksum-freshness -p bar")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness -p baz")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn dont_rebuild_based_on_plugins() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.1.1"
                edition = "2015"

                [workspace]
                members = ['baz']

                [dependencies]
                proc-macro-thing = { path = 'proc-macro-thing' }
            "#,
        )
        .file("src/lib.rs", "")
        .file(
            "proc-macro-thing/Cargo.toml",
            r#"
                [package]
                name = "proc-macro-thing"
                version = "0.1.1"
                edition = "2015"

                [lib]
                proc-macro = true

                [dependencies]
                qux = { path = '../qux' }
            "#,
        )
        .file("proc-macro-thing/src/lib.rs", "")
        .file(
            "baz/Cargo.toml",
            r#"
                [package]
                name = "baz"
                version = "0.1.1"
                edition = "2015"

                [dependencies]
                qux = { path = '../qux' }
            "#,
        )
        .file("baz/src/main.rs", "fn main() {}")
        .file("qux/Cargo.toml", &basic_manifest("qux", "0.1.1"))
        .file("qux/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness -p baz")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness -p bar")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn reuse_workspace_lib() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.1.1"
                edition = "2015"

                [workspace]

                [dependencies]
                baz = { path = 'baz' }
            "#,
        )
        .file("src/lib.rs", "")
        .file("baz/Cargo.toml", &basic_manifest("baz", "0.1.1"))
        .file("baz/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("test -Zchecksum-freshness -p baz -v --no-run")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] baz v0.1.1 ([ROOT]/foo/baz)
[RUNNING] `rustc --crate-name baz [..]
[FINISHED] `test` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[EXECUTABLE] `[ROOT]/foo/target/debug/deps/baz-[HASH][EXE]`

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn reuse_shared_build_dep() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                edition = "2015"

                [dependencies]
                shared = {path = "shared"}

                [workspace]
                members = ["shared", "bar"]
            "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file("shared/Cargo.toml", &basic_manifest("shared", "0.0.1"))
        .file("shared/src/lib.rs", "")
        .file(
            "bar/Cargo.toml",
            r#"
                [package]
                name = "bar"
                version = "0.0.1"
                edition = "2015"

                [build-dependencies]
                shared = { path = "../shared" }
            "#,
        )
        .file("bar/src/lib.rs", "")
        .file("bar/build.rs", "fn main() {}")
        .build();

    p.cargo("build -Zchecksum-freshness --workspace")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    // This should not recompile!
    p.cargo("build -Zchecksum-freshness -p foo -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FRESH] shared v0.0.1 ([ROOT]/foo/shared)
[FRESH] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn changing_rustflags_is_cached() {
    let p = project().file("src/lib.rs", "").build();

    // This isn't ever cached, we always have to recompile
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("RUSTFLAGS", "-C linker=cc")
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the rustflags changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the rustflags changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc [..] src/lib.rs [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("RUSTFLAGS", "-C linker=cc")
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the rustflags changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn reuse_panic_build_dep_test() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                edition = "2015"

                [build-dependencies]
                bar = { path = "bar" }

                [dev-dependencies]
                bar = { path = "bar" }

                [profile.dev]
                panic = "abort"
            "#,
        )
        .file("src/lib.rs", "")
        .file("build.rs", "fn main() {}")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "")
        .build();

    // Check that `bar` is not built twice. It is only needed once (without `panic`).
    p.cargo("test -Zchecksum-freshness --lib --no-run -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[LOCKING] 1 package to latest compatible version
[COMPILING] bar v0.0.1 ([ROOT]/foo/bar)
[RUNNING] `rustc --crate-name bar [..]
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name build_script_build [..]
[RUNNING] `[ROOT]/foo/target/debug/build/foo-[HASH]/build-script-build`
[RUNNING] `rustc --crate-name foo [..] src/lib.rs [..]--test[..]
[FINISHED] `test` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[EXECUTABLE] `[ROOT]/foo/target/debug/deps/foo-[HASH][EXE]`

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn reuse_panic_pm() {
    // foo(panic) -> bar(panic)
    // somepm(nopanic) -> bar(nopanic)
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                edition = "2015"

                [dependencies]
                bar = { path = "bar" }
                somepm = { path = "somepm" }

                [profile.dev]
                panic = "abort"
            "#,
        )
        .file("src/lib.rs", "extern crate bar;")
        .file("bar/Cargo.toml", &basic_manifest("bar", "0.0.1"))
        .file("bar/src/lib.rs", "")
        .file(
            "somepm/Cargo.toml",
            r#"
                [package]
                name = "somepm"
                version = "0.0.1"
                edition = "2015"

                [lib]
                proc-macro = true

                [dependencies]
                bar = { path = "../bar" }
            "#,
        )
        .file("somepm/src/lib.rs", "extern crate bar;")
        .build();

    // bar is built once without panic (for proc-macro) and once with (for the
    // normal dependency).
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(
            str![[r#"
[LOCKING] 2 packages to latest compatible versions
[COMPILING] bar v0.0.1 ([ROOT]/foo/bar)
[RUNNING] `rustc --crate-name bar [..] -C panic=abort [..]
[RUNNING] `rustc --crate-name bar [..]
[COMPILING] somepm v0.0.1 ([ROOT]/foo/somepm)
[RUNNING] `rustc --crate-name somepm [..]
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo [..] -C panic=abort [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]]
            .unordered(),
        )
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn bust_patched_dep() {
    Package::new("registry1", "0.1.0").publish();
    Package::new("registry2", "0.1.0")
        .dep("registry1", "0.1.0")
        .publish();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.0.1"
                edition = "2015"

                [dependencies]
                registry2 = "0.1.0"

                [patch.crates-io]
                registry1 = { path = "reg1new" }
            "#,
        )
        .file("src/lib.rs", "")
        .file("reg1new/Cargo.toml", &basic_manifest("registry1", "0.1.0"))
        .file("reg1new/src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    p.change_file("reg1new/src/lib.rs", "// modified");

    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[DIRTY] registry1 v0.1.0 ([ROOT]/foo/reg1new): file size changed (0 != 11) for `reg1new/src/lib.rs`
[COMPILING] registry1 v0.1.0 ([ROOT]/foo/reg1new)
[RUNNING] `rustc --crate-name registry1 [..]
[DIRTY] registry2 v0.1.0: the dependency registry1 was rebuilt
[COMPILING] registry2 v0.1.0
[RUNNING] `rustc --crate-name registry2 [..]
[DIRTY] foo v0.0.1 ([ROOT]/foo): the dependency registry2 was rebuilt
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FRESH] registry1 v0.1.0 ([ROOT]/foo/reg1new)
[FRESH] registry2 v0.1.0
[FRESH] foo v0.0.1 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn rebuild_on_mid_build_file_modification() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [workspace]
                members = ["root", "proc_macro_dep"]
            "#,
        )
        .file(
            "root/Cargo.toml",
            r#"
                [package]
                name = "root"
                version = "0.1.0"
                edition = "2015"
                authors = []

                [dependencies]
                proc_macro_dep = { path = "../proc_macro_dep" }
            "#,
        )
        .file(
            "root/src/lib.rs",
            r#"
                #[macro_use]
                extern crate proc_macro_dep;

                #[derive(Noop)]
                pub struct X;
            "#,
        )
        .file(
            "proc_macro_dep/Cargo.toml",
            r#"
                [package]
                name = "proc_macro_dep"
                version = "0.1.0"
                edition = "2015"
                authors = []

                [lib]
                proc-macro = true
            "#,
        )
        .file(
            "proc_macro_dep/src/lib.rs",
            &format!(
                r#"
                    extern crate proc_macro;

                    use std::io::Read;
                    use std::net::TcpStream;
                    use proc_macro::TokenStream;

                    #[proc_macro_derive(Noop)]
                    pub fn noop(_input: TokenStream) -> TokenStream {{
                        let mut stream = TcpStream::connect("{}").unwrap();
                        let mut v = Vec::new();
                        stream.read_to_end(&mut v).unwrap();
                        "".parse().unwrap()
                    }}
                "#,
                addr
            ),
        )
        .build();
    let root = p.root();

    let t = thread::spawn(move || {
        let socket = server.accept().unwrap().0;
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(root.join("root/src/lib.rs"))
            .unwrap();
        writeln!(file, "// modified").expect("Failed to append to root sources");
        drop(file);
        drop(socket);
        drop(server.accept().unwrap());
    });

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] proc_macro_dep v0.1.0 ([ROOT]/foo/proc_macro_dep)
[COMPILING] root v0.1.0 ([ROOT]/foo/root)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FRESH] proc_macro_dep v0.1.0 ([ROOT]/foo/proc_macro_dep)
[DIRTY] root v0.1.0 ([ROOT]/foo/root): file size changed (150 != 162) for `root/src/lib.rs`
[COMPILING] root v0.1.0 ([ROOT]/foo/root)
[RUNNING] `rustc --crate-name root [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    t.join().ok().unwrap();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn dirty_both_lib_and_test() {
    // This tests that all artifacts that depend on the results of a build
    // script will get rebuilt when the build script reruns, even for separate
    // commands. It does the following:
    //
    // 1. Project "foo" has a build script which will compile a small
    //    staticlib to link against. Normally this would use the `cc` crate,
    //    but here we just use rustc to avoid the `cc` dependency.
    // 2. Build the library.
    // 3. Build the unit test. The staticlib intentionally has a bad value.
    // 4. Rewrite the staticlib with the correct value.
    // 5. Build the library again.
    // 6. Build the unit test. This should recompile.

    let slib = |n| {
        format!(
            r#"
                #[no_mangle]
                pub extern "C" fn doit() -> i32 {{
                    return {};
                }}
            "#,
            n
        )
    };

    let p = project()
        .file(
            "src/lib.rs",
            r#"
                extern "C" {
                    fn doit() -> i32;
                }

                #[test]
                fn t1() {
                    assert_eq!(unsafe { doit() }, 1, "doit assert failure");
                }
            "#,
        )
        .file(
            "build.rs",
            r#"
                use std::env;
                use std::path::PathBuf;
                use std::process::Command;

                fn main() {
                    let rustc = env::var_os("RUSTC").unwrap();
                    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
                    assert!(
                        Command::new(rustc)
                            .args(&[
                                "--crate-type=staticlib",
                                "--out-dir",
                                out_dir.to_str().unwrap(),
                                "slib.rs"
                            ])
                            .status()
                            .unwrap()
                            .success(),
                        "slib build failed"
                    );
                    println!("cargo::rustc-link-lib=slib");
                    println!("cargo::rustc-link-search={}", out_dir.display());
                }
            "#,
        )
        .file("slib.rs", &slib(2))
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    // 2 != 1
    p.cargo("test -Zchecksum-freshness --lib")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_status(101)
        .with_stdout_data("...\n[..]doit assert failure[..]\n...")
        .run();

    // Fix the mistake.
    p.change_file("slib.rs", &slib(1));

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    // This should recompile with the new static lib, and the test should pass.
    p.cargo("test -Zchecksum-freshness --lib")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn script_fails_stay_dirty() {
    // Check if a script is aborted (such as hitting Ctrl-C) that it will re-run.
    // Steps:
    // 1. Build to establish fingerprints.
    // 2. Make a change that triggers the build script to re-run. Abort the
    //    script while it is running.
    // 3. Run the build again and make sure it re-runs the script.
    let p = project()
        .file(
            "build.rs",
            r#"
                mod helper;
                fn main() {
                    println!("cargo::rerun-if-changed=build.rs");
                    helper::doit();
                }
            "#,
        )
        .file("helper.rs", "pub fn doit() {}")
        .file("src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.change_file("helper.rs", r#"pub fn doit() {panic!("Crash!");}"#);
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data("...\n[..]Crash![..]\n...")
        .with_status(101)
        .run();
    // There was a bug where this second call would be "fresh".
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data("...\n[..]Crash![..]\n...")
        .with_status(101)
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn metadata_change_invalidates() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.1.0"
            edition = "2015"
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    for attr in &[
        "authors = [\"foo\"]",
        "description = \"desc\"",
        "homepage = \"https://example.com\"",
        "repository =\"https://example.com\"",
    ] {
        let mut file = OpenOptions::new()
            .write(true)
            .append(true)
            .open(p.root().join("Cargo.toml"))
            .unwrap();
        writeln!(file, "{}", attr).unwrap();
        p.cargo("build -Zchecksum-freshness")
            .masquerade_as_nightly_cargo(&["checksum-freshness"])
            .with_stderr_data(str![[r#"
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
            .run();
    }
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FRESH] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    assert_eq!(p.glob("target/debug/deps/libfoo-*.rlib").count(), 1);
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn edition_change_invalidates() {
    const MANIFEST: &str = r#"
        [package]
        name = "foo"
        version = "0.1.0"
    "#;
    let p = project()
        .file("Cargo.toml", MANIFEST)
        .file("src/lib.rs", "")
        .build();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.change_file("Cargo.toml", &format!("{}edition = \"2018\"", MANIFEST));
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.change_file(
        "Cargo.toml",
        &format!(
            r#"{}edition = "2018"
            [lib]
            edition = "2015"
            "#,
            MANIFEST
        ),
    );
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FRESH] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    assert_eq!(p.glob("target/debug/deps/libfoo-*.rlib").count(), 1);
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn rename_with_path_deps() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.5.0"
                edition = "2015"
                authors = []

                [dependencies]
                a = { path = 'a' }
            "#,
        )
        .file("src/lib.rs", "extern crate a; pub fn foo() { a::foo(); }")
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.5.0"
                edition = "2015"
                authors = []

                [dependencies]
                b = { path = 'b' }
            "#,
        )
        .file("a/src/lib.rs", "extern crate b; pub fn foo() { b::foo() }")
        .file(
            "a/b/Cargo.toml",
            r#"
                [package]
                name = "b"
                version = "0.5.0"
                edition = "2015"
                authors = []
            "#,
        )
        .file("a/b/src/lib.rs", "pub fn foo() { }");
    let p = p.build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    // Now rename the root directory and rerun `cargo run`. Not only should we
    // not build anything but we also shouldn't crash.
    let mut new = p.root();
    new.pop();
    new.push("foo2");

    fs::rename(p.root(), &new).unwrap();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .cwd(&new)
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn move_target_directory_with_path_deps() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.5.0"
                edition = "2015"
                authors = []

                [dependencies]
                a = { path = "a" }
            "#,
        )
        .file(
            "a/Cargo.toml",
            r#"
                [package]
                name = "a"
                version = "0.5.0"
                edition = "2015"
                authors = []
            "#,
        )
        .file("src/lib.rs", "extern crate a; pub use a::print_msg;")
        .file(
            "a/build.rs",
            r###"
                use std::env;
                use std::fs;
                use std::path::Path;

                fn main() {
                    println!("cargo::rerun-if-changed=build.rs");
                    let out_dir = env::var("OUT_DIR").unwrap();
                    let dest_path = Path::new(&out_dir).join("hello.rs");
                    fs::write(&dest_path, r#"
                        pub fn message() -> &'static str {
                            "Hello, World!"
                        }
                    "#).unwrap();
                }
            "###,
        )
        .file(
            "a/src/lib.rs",
            r#"
            include!(concat!(env!("OUT_DIR"), "/hello.rs"));
            pub fn print_msg() { message(); }
            "#,
        );
    let p = p.build();

    let mut parent = p.root();
    parent.pop();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    let new_target = p.root().join("target2");
    fs::rename(p.root().join("target"), &new_target).unwrap();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("CARGO_TARGET_DIR", &new_target)
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn rerun_if_changes() {
    let p = project()
        .file(
            "build.rs",
            r#"
                fn main() {
                    println!("cargo::rerun-if-env-changed=FOO");
                    if std::env::var("FOO").is_ok() {
                        println!("cargo::rerun-if-env-changed=BAR");
                    }
                }
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", "1")
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the env variable FOO changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `[ROOT]/foo/target/debug/build/foo-[HASH]/build-script-build`
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", "1")
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", "1")
        .env("BAR", "1")
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the env variable BAR changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `[ROOT]/foo/target/debug/build/foo-[HASH]/build-script-build`
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", "1")
        .env("BAR", "1")
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("BAR", "2")
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the env variable FOO changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `[ROOT]/foo/target/debug/build/foo-[HASH]/build-script-build`
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("BAR", "2")
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn channel_shares_filenames() {
    // Test that different "nightly" releases use the same output filename.

    // Create separate rustc binaries to emulate running different toolchains.
    let nightly1 = format!(
        "\
rustc 1.44.0-nightly (38114ff16 2020-03-21)
binary: rustc
commit-hash: 38114ff16e7856f98b2b4be7ab4cd29b38bed59a
commit-date: 2020-03-21
host: {}
release: 1.44.0-nightly
LLVM version: 9.0
",
        rustc_host()
    );

    let nightly2 = format!(
        "\
rustc 1.44.0-nightly (a5b09d354 2020-03-31)
binary: rustc
commit-hash: a5b09d35473615e7142f5570f5c5fad0caf68bd2
commit-date: 2020-03-31
host: {}
release: 1.44.0-nightly
LLVM version: 9.0
",
        rustc_host()
    );

    let beta1 = format!(
        "\
rustc 1.43.0-beta.3 (4c587bbda 2020-03-25)
binary: rustc
commit-hash: 4c587bbda04ab55aaf56feab11dfdfe387a85d7a
commit-date: 2020-03-25
host: {}
release: 1.43.0-beta.3
LLVM version: 9.0
",
        rustc_host()
    );

    let beta2 = format!(
        "\
rustc 1.42.0-beta.5 (4e1c5f0e9 2020-02-28)
binary: rustc
commit-hash: 4e1c5f0e9769a588b91c977e3d81e140209ef3a2
commit-date: 2020-02-28
host: {}
release: 1.42.0-beta.5
LLVM version: 9.0
",
        rustc_host()
    );

    let stable1 = format!(
        "\
rustc 1.42.0 (b8cedc004 2020-03-09)
binary: rustc
commit-hash: b8cedc00407a4c56a3bda1ed605c6fc166655447
commit-date: 2020-03-09
host: {}
release: 1.42.0
LLVM version: 9.0
",
        rustc_host()
    );

    let stable2 = format!(
        "\
rustc 1.41.1 (f3e1a954d 2020-02-24)
binary: rustc
commit-hash: f3e1a954d2ead4e2fc197c7da7d71e6c61bad196
commit-date: 2020-02-24
host: {}
release: 1.41.1
LLVM version: 9.0
",
        rustc_host()
    );

    let compiler = project()
        .at("compiler")
        .file("Cargo.toml", &basic_manifest("compiler", "0.1.0"))
        .file(
            "src/main.rs",
            r#"
            fn main() {
                if std::env::args_os().any(|a| a == "-vV") {
                    print!("{}", env!("FUNKY_VERSION_TEST"));
                    return;
                }
                let mut cmd = std::process::Command::new("rustc");
                cmd.args(std::env::args_os().skip(1));
                assert!(cmd.status().unwrap().success());
            }
            "#,
        )
        .build();

    let makeit = |version, vv| {
        // Force a rebuild.
        compiler.target_debug_dir().join("deps").rm_rf();
        compiler
            .cargo("build -Zchecksum-freshness")
            .masquerade_as_nightly_cargo(&["checksum-freshness"])
            .env("FUNKY_VERSION_TEST", vv)
            .run();
        fs::rename(compiler.bin("compiler"), compiler.bin(version)).unwrap();
    };
    makeit("nightly1", nightly1);
    makeit("nightly2", nightly2);
    makeit("beta1", beta1);
    makeit("beta2", beta2);
    makeit("stable1", stable1);
    makeit("stable2", stable2);

    // Run `cargo check` with different rustc versions to observe its behavior.
    let p = project().file("src/lib.rs", "").build();

    // Runs `cargo check` and returns the rmeta filename created.
    // Checks that the freshness matches the given value.
    let check = |version, fresh| -> String {
        let output = p
            .cargo("check -Zchecksum-freshness --message-format=json")
            .masquerade_as_nightly_cargo(&["checksum-freshness"])
            .env("RUSTC", compiler.bin(version))
            .run();
        // Collect the filenames generated.
        let mut artifacts: Vec<_> = std::str::from_utf8(&output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).unwrap();
                if value["reason"].as_str().unwrap() == "compiler-artifact" {
                    assert_eq!(value["fresh"].as_bool().unwrap(), fresh);
                    let filenames = value["filenames"].as_array().unwrap();
                    assert_eq!(filenames.len(), 1);
                    Some(filenames[0].to_string())
                } else {
                    None
                }
            })
            .collect();
        // Should only generate one rmeta file.
        assert_eq!(artifacts.len(), 1);
        artifacts.pop().unwrap()
    };

    let nightly1_name = check("nightly1", false);
    assert_eq!(check("nightly1", true), nightly1_name);
    assert_eq!(check("nightly2", false), nightly1_name); // same as before
    assert_eq!(check("nightly2", true), nightly1_name);
    // Should rebuild going back to nightly1.
    assert_eq!(check("nightly1", false), nightly1_name);

    let beta1_name = check("beta1", false);
    assert_ne!(beta1_name, nightly1_name);
    assert_eq!(check("beta1", true), beta1_name);
    assert_eq!(check("beta2", false), beta1_name); // same as before
    assert_eq!(check("beta2", true), beta1_name);
    // Should rebuild going back to beta1.
    assert_eq!(check("beta1", false), beta1_name);

    let stable1_name = check("stable1", false);
    assert_ne!(stable1_name, nightly1_name);
    assert_ne!(stable1_name, beta1_name);
    let stable2_name = check("stable2", false);
    assert_ne!(stable1_name, stable2_name);
    // Check everything is fresh.
    assert_eq!(check("stable1", true), stable1_name);
    assert_eq!(check("stable2", true), stable2_name);
    assert_eq!(check("beta1", true), beta1_name);
    assert_eq!(check("nightly1", true), nightly1_name);
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn linking_interrupted() {
    // Interrupt during the linking phase shouldn't leave test executable as "fresh".

    // This is used to detect when linking starts, then to pause the linker so
    // that the test can kill cargo.
    let link_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let link_addr = link_listener.local_addr().unwrap();

    // This is used to detect when rustc exits.
    let rustc_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let rustc_addr = rustc_listener.local_addr().unwrap();

    // Create a linker that we can interrupt.
    let linker = project()
        .at("linker")
        .file("Cargo.toml", &basic_manifest("linker", "1.0.0"))
        .file(
            "src/main.rs",
            &r#"
            fn main() {
                // Figure out the output filename.
                let output = match std::env::args().find(|a| a.starts_with("/OUT:")) {
                    Some(s) => s[5..].to_string(),
                    None => {
                        let mut args = std::env::args();
                        loop {
                            if args.next().unwrap() == "-o" {
                                break;
                            }
                        }
                        args.next().unwrap()
                    }
                };
                std::fs::remove_file(&output).unwrap();
                std::fs::write(&output, "").unwrap();
                // Tell the test that we are ready to be interrupted.
                let mut socket = std::net::TcpStream::connect("__ADDR__").unwrap();
                // Wait for the test to kill us.
                std::thread::sleep(std::time::Duration::new(60, 0));
            }
            "#
            .replace("__ADDR__", &link_addr.to_string()),
        )
        .build();
    linker
        .cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    // Create a wrapper around rustc that will tell us when rustc is finished.
    let rustc = project()
        .at("rustc-waiter")
        .file("Cargo.toml", &basic_manifest("rustc-waiter", "1.0.0"))
        .file(
            "src/main.rs",
            &r#"
            fn main() {
                let mut conn = None;
                // Check for a normal build (not -vV or --print).
                if std::env::args().any(|arg| arg == "t1") {
                    // Tell the test that rustc has started.
                    conn = Some(std::net::TcpStream::connect("__ADDR__").unwrap());
                }
                let status = std::process::Command::new("rustc")
                    .args(std::env::args().skip(1))
                    .status()
                    .expect("rustc to run");
                std::process::exit(status.code().unwrap_or(1));
            }
            "#
            .replace("__ADDR__", &rustc_addr.to_string()),
        )
        .build();
    rustc
        .cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    // Build it once so that the fingerprint gets saved to disk.
    let p = project()
        .file("src/lib.rs", "")
        .file("tests/t1.rs", "")
        .build();
    p.cargo("test -Zchecksum-freshness --test t1 --no-run")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();

    // Make a change, start a build, then interrupt it.
    p.change_file("src/lib.rs", "// modified");
    let linker_env = format!("CARGO_TARGET_{}_LINKER", rustc_host_env());
    // NOTE: This assumes that the paths to the linker or rustc are not in the
    // fingerprint. But maybe they should be?
    let mut cmd = p
        .cargo("test -Zchecksum-freshness --test t1 --no-run")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env(&linker_env, linker.bin("linker"))
        .env("RUSTC", rustc.bin("rustc-waiter"))
        .build_command();
    let mut child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("__CARGO_TEST_SETSID_PLEASE_DONT_USE_ELSEWHERE", "1")
        .spawn()
        .unwrap();
    // Wait for rustc to start.
    let mut rustc_conn = rustc_listener.accept().unwrap().0;
    // Wait for linking to start.
    drop(link_listener.accept().unwrap());

    // Interrupt the child.
    death::ctrl_c(&mut child);
    assert!(!child.wait().unwrap().success());
    // Wait for rustc to exit. If we don't wait, then the command below could
    // start while rustc is still being torn down.
    let mut buf = [0];
    drop(rustc_conn.read_exact(&mut buf));

    // Build again, shouldn't be fresh.
    p.cargo("test -Zchecksum-freshness --test t1 -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the config settings changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo [..]
[RUNNING] `rustc --crate-name t1 [..]
[FINISHED] `test` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s
[RUNNING] `[ROOT]/foo/target/debug/deps/t1-[HASH][EXE]`

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
#[cfg(all(target_arch = "x86_64", target_os = "windows", target_env = "msvc"))]
fn lld_is_fresh() {
    // Check for bug when using lld linker that it remains fresh with dylib.
    let p = project()
        .file(
            ".cargo/config.toml",
            r#"
                [target.x86_64-pc-windows-msvc]
                linker = "rust-lld"
                rustflags = ["-C", "link-arg=-fuse-ld=lld"]
            "#,
        )
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2015"

                [lib]
                crate-type = ["dylib"]
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FRESH] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn env_in_code_causes_rebuild() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2015"
            "#,
        )
        .file(
            "src/main.rs",
            r#"
                fn main() {
                    println!("{:?}", option_env!("FOO"));
                    println!("{:?}", option_env!("FOO\nBAR"));
                }
            "#,
        )
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env_remove("FOO")
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env_remove("FOO")
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", "bar")
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.1.0 ([ROOT]/foo): the environment variable FOO changed
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", "bar")
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", "baz")
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.1.0 ([ROOT]/foo): the environment variable FOO changed
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", "baz")
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env_remove("FOO")
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.1.0 ([ROOT]/foo): the environment variable FOO changed
[COMPILING] foo v0.1.0 ([ROOT]/foo)
[RUNNING] `rustc [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env_remove("FOO")
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    let interesting = " #!$\nabc\r\\\t\u{8}\r\n";
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", interesting)
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO", interesting)
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO\nBAR", interesting)
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("FOO\nBAR", interesting)
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn env_build_script_no_rebuild() {
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2015"
            "#,
        )
        .file(
            "build.rs",
            r#"
                fn main() {
                    println!("cargo::rustc-env=FOO=bar");
                }
            "#,
        )
        .file(
            "src/main.rs",
            r#"
                fn main() {
                    println!("{:?}", env!("FOO"));
                }
            "#,
        )
        .build();

    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn changing_linker() {
    // Changing linker should rebuild.
    let p = project().file("src/main.rs", "fn main() {}").build();
    p.cargo("build -Zchecksum-freshness")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .run();
    let linker_env = format!("CARGO_TARGET_{}_LINKER", rustc_host_env());
    p.cargo("build -Zchecksum-freshness --verbose")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env(&linker_env, "nonexistent-linker")
        .with_status(101)
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.0.1 ([ROOT]/foo): the config settings changed
[COMPILING] foo v0.0.1 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo [..] -C linker=nonexistent-linker [..]`
[ERROR] linker `nonexistent-linker` not found
...
"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn verify_source_before_recompile() {
    Package::new("bar", "0.1.0")
        .file("src/lib.rs", "")
        .publish();
    let p = project()
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2015"

                [dependencies]
                bar = "0.1.0"
            "#,
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("vendor --respect-source-config").run();
    p.change_file(
        ".cargo/config.toml",
        r#"
            [source.crates-io]
            replace-with = 'vendor'

            [source.vendor]
            directory = 'vendor'
        "#,
    );
    // Sanity check: vendoring works correctly.
    p.cargo("check -Zchecksum-freshness --verbose")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[CHECKING] bar v0.1.0
[RUNNING] `rustc --crate-name bar [..] [ROOT]/foo/vendor/bar/src/lib.rs [..]
[CHECKING] foo v0.1.0 ([ROOT]/foo)
[RUNNING] `rustc --crate-name foo [..] src/lib.rs [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    // Now modify vendored crate.
    p.change_file(
        "vendor/bar/src/lib.rs",
        r#"compile_error!("You shall not pass!");"#,
    );
    // Should ignore modified sources without any recompile.
    p.cargo("check -Zchecksum-freshness --verbose")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .with_stderr_data(str![[r#"
[FRESH] bar v0.1.0
[FRESH] foo v0.1.0 ([ROOT]/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();

    // Add a `RUSTFLAGS` to trigger a recompile.
    //
    // Cargo should refuse to build because of checksum verification failure.
    // Cargo shouldn't recompile dependency `bar`.
    p.cargo("check -Zchecksum-freshness --verbose")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("RUSTFLAGS", "-W warnings")
        .with_status(101)
        .with_stderr_data(str![[r#"
[ERROR] the listed checksum of `[ROOT]/foo/vendor/bar/src/lib.rs` has changed:
expected: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
actual:   66e843918c1d4ea8231af814f9f958958808249d4407de01114acb730ecd9bdf

directory sources are not intended to be edited, if modifications are required then it is recommended that `[patch]` is used with a forked copy of the source

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn skip_checksum_check_in_selected_cargo_home_subdirs() {
    let p = project()
        .at("cargo_home/registry/foo")
        .file("Cargo.toml", &basic_lib_manifest("foo"))
        .file("src/lib.rs", "")
        .build();
    let project_root = p.root();
    let cargo_home = project_root.parent().unwrap().parent().unwrap();
    p.cargo("check -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("CARGO_HOME", &cargo_home)
        .with_stderr_data(str![[r#"
[CHECKING] foo v0.5.0 ([ROOT]/cargo_home/registry/foo)
[RUNNING] `rustc --crate-name foo [..] src/lib.rs [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.change_file("src/lib.rs", "illegal syntax");
    p.cargo("check -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("CARGO_HOME", &cargo_home)
        .with_stderr_data(str![[r#"
[FRESH] foo v0.5.0 ([ROOT]/cargo_home/registry/foo)
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
}

#[cargo_test(nightly, reason = "requires -Zchecksum-hash-algorithm")]
fn use_checksum_cache_in_cargo_home() {
    let p = project()
        .at("cargo_home/foo")
        .file("Cargo.toml", &basic_lib_manifest("foo"))
        .file("src/lib.rs", "")
        .build();
    let project_root = p.root();
    let cargo_home = project_root.parent().unwrap();
    p.cargo("check -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("CARGO_HOME", &cargo_home)
        .with_stderr_data(str![[r#"
[CHECKING] foo v0.5.0 ([ROOT]/cargo_home/foo)
[RUNNING] `rustc --crate-name foo [..] src/lib.rs [..] src/lib.rs [..]
[FINISHED] `dev` profile [unoptimized + debuginfo] target(s) in [ELAPSED]s

"#]])
        .run();
    p.change_file("src/lib.rs", "illegal syntax");
    p.cargo("check -Zchecksum-freshness -v")
        .masquerade_as_nightly_cargo(&["checksum-freshness"])
        .env("CARGO_HOME", &cargo_home)
        .with_status(101)
        .with_stderr_data(str![[r#"
[DIRTY] foo v0.5.0 ([ROOT]/cargo_home/foo): file size changed (0 != 14) for `src/lib.rs`
[CHECKING] foo v0.5.0 ([ROOT]/cargo_home/foo)
[RUNNING] `rustc --crate-name foo [..] src/lib.rs [..]
...
[ERROR] could not compile `foo` (lib) due to 1 previous error
...
"#]])
        .run();
}