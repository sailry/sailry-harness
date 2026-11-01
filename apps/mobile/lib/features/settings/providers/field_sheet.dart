import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import 'api.dart' as provider_api;
import 'parts.dart';

/// Edits one text value. [check] returns a localization key for invalid
/// input; empty input is rejected unless [optional].
Future<String?> editText(
  BuildContext context, {
  required String title,
  String initial = '',
  String? Function(String value)? check,
  bool optional = false,
  bool secret = false,
  bool numeric = false,
  String? hint,
}) => showAppSheet<String>(
  context,
  title,
  child: _TextEditor(
    title: title,
    initial: initial,
    check: check,
    optional: optional,
    secret: secret,
    numeric: numeric,
    hint: hint,
  ),
);

class _TextEditor extends StatefulWidget {
  const _TextEditor({
    required this.title,
    required this.initial,
    required this.check,
    required this.optional,
    required this.secret,
    required this.numeric,
    required this.hint,
  });
  final String title;
  final String initial;
  final String? Function(String value)? check;
  final bool optional;
  final bool secret;
  final bool numeric;
  final String? hint;

  @override
  State<_TextEditor> createState() => _TextEditorState();
}

class _TextEditorState extends State<_TextEditor> {
  late final _controller = TextEditingController(text: widget.initial);
  late bool _hidden = widget.secret;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _save() {
    final value = _controller.text.trim();
    final problem = value.isEmpty
        ? (widget.optional ? null : 'settingsRequired')
        : widget.check?.call(value);
    if (accept(context, problem)) Navigator.pop(context, value);
  }

  @override
  Widget build(BuildContext context) => FormBody(
    children: [
      TextField(
        controller: _controller,
        autofocus: true,
        obscureText: _hidden,
        autocorrect: !widget.secret,
        enableSuggestions: !widget.secret,
        keyboardType: widget.numeric ? TextInputType.number : null,
        inputFormatters: widget.numeric
            ? [FilteringTextInputFormatter.digitsOnly]
            : null,
        onSubmitted: (_) => _save(),
        decoration: InputDecoration(
          labelText: widget.title,
          hintText: widget.hint,
          suffixIcon: widget.secret
              ? IconButton(
                  tooltip: context.tr(
                    _hidden ? 'providerShowKey' : 'providerHideKey',
                  ),
                  onPressed: () => setState(() => _hidden = !_hidden),
                  icon: const AppIcon('eye', size: 18),
                )
              : null,
        ),
      ),
      FilledButton(onPressed: _save, child: Text(context.tr('save'))),
    ],
  );
}

/// Picks an API format; returns null when dismissed.
Future<String?> pickApi(BuildContext context, String current) =>
    showAppSheet<String>(
      context,
      context.tr('settingsApi'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final api in provider_api.choices(current))
              ListTile(
                title: Text(apiLabel(context, api)),
                trailing: api == current ? const AppIcon('check') : null,
                onTap: () => Navigator.pop(context, api),
              ),
          ],
        ),
      ),
    );

String apiLabel(BuildContext context, String api) {
  final key = provider_api.labelKey(api);
  return key == null ? api : context.tr(key);
}
