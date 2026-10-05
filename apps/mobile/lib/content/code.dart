import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:re_highlight/re_highlight.dart';
import 'package:re_highlight/languages/all.dart';

import '../l10n/strings.dart';
import '../ui/kit.dart';
import '../ui/toast.dart';

final _highlight = Highlight()..registerLanguages(builtinAllLanguages);

String languageFor(String path) {
  final name = path.split('/').last.toLowerCase();
  return switch (name.split('.').last) {
    'rs' => 'rust',
    'py' => 'python',
    'js' || 'jsx' || 'mjs' => 'javascript',
    'ts' || 'tsx' => 'typescript',
    'yml' => 'yaml',
    'sh' => 'bash',
    'md' => 'markdown',
    'h' => 'c',
    'hpp' || 'cc' => 'cpp',
    _ => name.contains('.') ? name.split('.').last : name,
  };
}

class CodeText extends StatefulWidget {
  const CodeText(this.text, {super.key, this.language, this.style});
  final String text;
  final String? language;
  final TextStyle? style;
  @override
  State<CodeText> createState() => _CodeTextState();
}

class _CodeTextState extends State<CodeText> {
  HighlightResult? _result;
  @override
  void initState() {
    super.initState();
    _parse();
  }

  @override
  void didUpdateWidget(CodeText oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.text != oldWidget.text ||
        widget.language != oldWidget.language) {
      _parse();
    }
  }

  void _parse() {
    _result = null;
    final language = widget.language;
    if (language != null && _highlight.getLanguage(language) != null) {
      _result = _highlight.highlight(code: widget.text, language: language);
    }
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final style = TextStyle(
      fontFamily: 'monospace',
      fontSize: 13,
      height: 1.6,
      color: colors.onSurface,
    ).merge(widget.style);
    final renderer = TextSpanRenderer(style, {
      'keyword': TextStyle(color: colors.secondary),
      'string': TextStyle(color: colors.tertiary),
      'number': TextStyle(color: colors.secondary),
      'comment': TextStyle(color: colors.onSurfaceVariant),
      'title': TextStyle(color: colors.primary, fontWeight: FontWeight.w600),
      'literal': TextStyle(color: colors.secondary),
      'built_in': TextStyle(color: colors.tertiary),
    });
    _result?.render(renderer);
    return SelectableText.rich(
      renderer.span ?? TextSpan(text: widget.text, style: style),
    );
  }
}

class CopyTextButton extends StatelessWidget {
  const CopyTextButton(this.text, {super.key});
  final String text;
  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: context.tr('copy'),
    icon: const AppIcon('copy', size: 16),
    onPressed: () async {
      var message = 'copied';
      try {
        await Clipboard.setData(ClipboardData(text: text));
      } catch (_) {
        message = 'copyFailed';
      }
      if (context.mounted) showToast(context, context.tr(message));
    },
  );
}

class CodeBlock extends StatelessWidget {
  const CodeBlock(
    this.text, {
    super.key,
    this.language,
    this.title,
    this.style,
  });
  final String text;
  final String? language;
  final String? title;
  final TextStyle? style;
  @override
  Widget build(BuildContext context) => Surface(
    padding: EdgeInsets.zero,
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.only(left: 12),
          child: Row(
            children: [
              Expanded(
                child: Text(title ?? language ?? context.tr('codePlain')),
              ),
              CopyTextButton(text),
            ],
          ),
        ),
        const Divider(height: 1),
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 360),
          child: SingleChildScrollView(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.all(12),
              child: CodeText(text, language: language, style: style),
            ),
          ),
        ),
      ],
    ),
  );
}
