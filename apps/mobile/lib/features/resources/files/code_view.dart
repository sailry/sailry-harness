import 'package:flutter/material.dart';

import '../../../content/code.dart';

/// Full-width highlighted source with a dim line-number gutter.
class CodeView extends StatelessWidget {
  const CodeView(this.source, {super.key, this.language});
  final String source;
  final String? language;

  static const _style = TextStyle(
    fontFamily: 'monospace',
    fontSize: 13,
    height: 1.6,
  );

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final count = '\n'.allMatches(source).length + 1;
    return SingleChildScrollView(
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // One text block keeps gutter rows aligned with the unwrapped code.
          Padding(
            padding: const EdgeInsets.only(left: 12, right: 14),
            child: Text(
              List.generate(count, (index) => '${index + 1}').join('\n'),
              textAlign: TextAlign.right,
              style: _style.copyWith(
                color: colors.onSurfaceVariant.withValues(alpha: .5),
              ),
            ),
          ),
          Expanded(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.only(right: 16),
              child: CodeText(source, language: language, style: _style),
            ),
          ),
        ],
      ),
    );
  }
}
