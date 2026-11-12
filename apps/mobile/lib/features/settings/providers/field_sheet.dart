import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';
import 'api.dart' as provider_api;
import 'parts.dart';

/// Keeps the draft until [onSave] succeeds. [check] returns a localization key
/// for invalid input; empty input is rejected unless [optional].
Future<void> editText(
  BuildContext context, {
  required String title,
  required Future<bool> Function(String value) onSave,
  String initial = '',
  String? Function(String value)? check,
  bool optional = false,
  bool secret = false,
  bool numeric = false,
  String? hint,
}) => showAppSheet<void>(
  context,
  title,
  child: _TextEditor(
    title: title,
    onSave: onSave,
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
    required this.onSave,
    required this.initial,
    required this.check,
    required this.optional,
    required this.secret,
    required this.numeric,
    required this.hint,
  });
  final String title;
  final Future<bool> Function(String value) onSave;
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
  bool _busy = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_busy) return;
    final value = _controller.text.trim();
    final problem = value.isEmpty
        ? (widget.optional ? null : 'settingsRequired')
        : widget.check?.call(value);
    if (!accept(context, problem)) return;
    final route = ModalRoute.of(context);
    setState(() => _busy = true);
    final saved = await attempt(context, () => widget.onSave(value));
    if (!mounted) return;
    if (saved && route?.isCurrent == true) {
      Navigator.pop(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => LoadingOverlay(
    loading: _busy,
    child: FormBody(
      children: [
        TextField(
          controller: _controller,
          enabled: !_busy,
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
                    onPressed: _busy
                        ? null
                        : () => setState(() => _hidden = !_hidden),
                    icon: const AppIcon('eye', size: 18),
                  )
                : null,
          ),
        ),
        FilledButton(
          onPressed: _busy ? null : _save,
          child: Text(context.tr('save')),
        ),
      ],
    ),
  );
}

/// Keeps the selected API visible until its Node save succeeds.
Future<void> pickApi(
  BuildContext context,
  String current, {
  required Future<bool> Function(String api) onSave,
}) => showAppSheet<void>(
  context,
  context.tr('settingsApi'),
  child: _ApiPicker(current: current, onSave: onSave),
);

class _ApiPicker extends StatefulWidget {
  const _ApiPicker({required this.current, required this.onSave});
  final String current;
  final Future<bool> Function(String api) onSave;

  @override
  State<_ApiPicker> createState() => _ApiPickerState();
}

class _ApiPickerState extends State<_ApiPicker> {
  late String _selected = widget.current;
  bool _busy = false;

  Future<void> _save(String api) async {
    if (_busy) return;
    final route = ModalRoute.of(context);
    setState(() {
      _selected = api;
      _busy = true;
    });
    final saved = await attempt(context, () => widget.onSave(api));
    if (!mounted) return;
    if (saved && route?.isCurrent == true) {
      Navigator.pop(context);
    } else {
      setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => LoadingOverlay(
    loading: _busy,
    child: Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        for (final api in provider_api.choices(widget.current))
          ListTile(
            title: Text(apiLabel(context, api)),
            trailing: api == _selected ? const AppIcon('check') : null,
            onTap: _busy ? null : () => _save(api),
          ),
      ],
    ),
  );
}

String apiLabel(BuildContext context, String api) {
  final key = provider_api.labelKey(api);
  return key == null ? api : context.tr(key);
}
