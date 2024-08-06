/* SPDX-License-Identifier: GPL-3.0-or-later */

use add_determinism::handlers::jar;

use super::{make_handler, test_corpus_file};

#[test]
fn test_jar_hello() {
    let filename = "tests/cases/jars/hello.jar";

    let jar = make_handler(1234563300, false, jar::Jar::boxed).unwrap();
    test_corpus_file(jar, filename);
}
