"""Edits of verifier 133's probes, each on a checkout that a step restores with git afterwards.

usage: edit.py probe            copy v133.rs into the test tree and name it in mod.rs
       edit.py decoy            build.rs builds the pinned source without the statements, the stops and the mark,
                                with a default cache size of -1234 as the decoy's own sign
"""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MOD = 'crates/halvo-sqlcipher/tests/page_gap/mod.rs'
BUILD = 'crates/halvo-sqlcipher/build.rs'


def read(path):
    return open(path, newline='', encoding='utf-8').read()


def write(path, text):
    open(path, 'w', newline='', encoding='utf-8').write(text)


mode = sys.argv[1]
if mode == 'probe':
    shutil.copyfile(os.path.join(HERE, 'v133.rs'), 'crates/halvo-sqlcipher/tests/page_gap/v133.rs')
    text = read(MOD)
    line = 'mod table;'
    assert text.count(line) == 1, text.count(line)
    if 'mod v133;' not in text:
        text = text.replace(line, line + '\nmod v133;')
    write(MOD, text)
    print('edit.py: the probe module is in the test tree')
elif mode == 'decoy':
    text = read(BUILD)
    a = '    ("HAVE_ISNAN", None),'
    b = '    let mut ends = Vec::new();'
    assert text.count(a) == 1 and text.count(b) == 1
    text = text.replace(a, a + '\n    ("SQLITE_DEFAULT_CACHE_SIZE", Some("-1234")),')
    text = text.replace(b, '    if true {\n        return source.to_vec();\n    }\n' + b)
    write(BUILD, text)
    print('edit.py: build.rs builds the decoy')
else:
    sys.exit(64)
