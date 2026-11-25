"""Unit tests for deb_copyright.py: python3 -m unittest discover -s tools/qemu -v"""

import unittest

from deb_copyright import pointers, undefined

HEAD = "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\n"


class Pointers(unittest.TestCase):
    def test_follows_each_pointer_once(self):
        text = ("see /usr/share/common-licenses/LGPL-2.1.\n"
                "and /usr/share/common-licenses/GPL-2 or /usr/share/common-licenses/LGPL-2.1\n")
        self.assertEqual(pointers(text), ["LGPL-2.1", "GPL-2"])

    def test_no_pointer(self):
        self.assertEqual(pointers("Permission is hereby granted"), [])


class Undefined(unittest.TestCase):
    def test_short_name_alone_fails(self):
        text = HEAD + "Files: *\nCopyright: someone\nLicense: LGPL-2.1+\n"
        self.assertEqual(undefined(text), ["LGPL-2.1+"])

    def test_pointer_defines_it(self):
        text = (HEAD + "Files: *\nLicense: LGPL-2.1+\n\nLicense: LGPL-2.1+\n"
                " On Debian systems, see /usr/share/common-licenses/LGPL-2.1.\n")
        self.assertEqual(undefined(text), [])

    def test_body_defines_it(self):
        text = (HEAD + "Files: *\nLicense: GPL-2+ or BSD-2-clause\n\n"
                "License: BSD-2-clause\n Redistribution and use in source and binary forms\n\n"
                "License: GPL-2+\n see /usr/share/common-licenses/GPL-2\n")
        self.assertEqual(undefined(text), [])

    def test_one_of_two_missing(self):
        text = (HEAD + "Files: *\nLicense: GPL-2+ or BSD-2-clause\n\n"
                "License: GPL-2+\n see /usr/share/common-licenses/GPL-2\n")
        self.assertEqual(undefined(text), ["BSD-2-clause"])

    def test_old_format_is_not_parsed(self):
        self.assertEqual(undefined("License: LGPL\n"), [])


if __name__ == "__main__":
    unittest.main()
