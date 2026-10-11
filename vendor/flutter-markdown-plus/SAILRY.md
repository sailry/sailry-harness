# Source and local changes

Source: `foresightmobile/flutter_markdown_plus`, pub.dev release `1.0.12`.
Archive: `https://pub.dev/api/archives/flutter_markdown_plus-1.0.12.tar.gz`.
SHA-256: `fce641d6c2106cc495de1cd603f97a4f9d615a97a64011928ded380dbdade935`.
The six library files, package manifest and BSD license are retained from that
release. Mobile selects this exact package through its path override and lockfile.

The upstream builder forces paragraph strut height with no public override,
clipping tall inline formulas in WidgetSpan. The local change recursively finds
WidgetSpan in the paragraph's spans and allows those paragraphs to expand to
fit their inline widgets. Plain text retains the upstream fixed strut height;
parsing, selection, links, lists, tables and widget composition stay upstream.
Mobile geometry and native-rendering regressions cover the patched behavior.
