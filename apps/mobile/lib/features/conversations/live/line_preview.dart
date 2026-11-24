import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';

/// Lines a long preview shows before Show all.
const previewLines = 12;

/// A long text or diff in the transcript shows its first [previewLines] lines
/// and opens in place, so a card never holds a nested vertical scroll that
/// would stop a finger's drag inside the transcript.
class LinePreview extends StatefulWidget {
  const LinePreview({super.key, required this.lines, required this.builder});

  /// The content's total lines.
  final int lines;

  /// Builds the content, limited to that many lines, or whole for null.
  final Widget Function(BuildContext context, int? limit) builder;

  @override
  State<LinePreview> createState() => _LinePreviewState();
}

class _LinePreviewState extends State<LinePreview> {
  bool _full = false;

  @override
  Widget build(BuildContext context) {
    final long = widget.lines > previewLines;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        widget.builder(context, long && !_full ? previewLines : null),
        if (long)
          Align(
            alignment: Alignment.centerLeft,
            child: TextButton(
              onPressed: () => setState(() => _full = !_full),
              child: Text(
                _full
                    ? context.tr('contentShowLess')
                    : context
                          .tr('contentShowAll')
                          .replaceAll('{count}', '${widget.lines}'),
              ),
            ),
          ),
      ],
    );
  }
}
