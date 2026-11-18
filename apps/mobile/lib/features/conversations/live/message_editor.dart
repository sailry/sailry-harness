import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../ui/kit.dart';

class MessageEditor extends StatefulWidget {
  const MessageEditor({
    super.key,
    required this.initial,
    required this.onChanged,
    required this.onSubmit,
    required this.onCheck,
    required this.isPending,
  });
  final String initial;
  final ValueChanged<String> onChanged;
  final bool Function() isPending;
  final Future<bool> Function(String) onSubmit;
  final Future<bool> Function() onCheck;
  @override
  State<MessageEditor> createState() => _MessageEditorState();
}

class _MessageEditorState extends State<MessageEditor> {
  late final _text = TextEditingController(text: widget.initial);
  bool _busy = false;
  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  VoidCallback _submit(bool checking) => () async {
    setState(() => _busy = true);
    final accepted = checking
        ? await widget.onCheck()
        : await widget.onSubmit(_text.text);
    if (!mounted) return;
    setState(() => _busy = false);
    if (accepted) Navigator.pop(context);
  };

  @override
  Widget build(BuildContext context) => PopScope(
    canPop: !_busy,
    child: PageFrame(
      title: context.tr('messageEdit'),
      backEnabled: !_busy,
      loading: _busy,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(context.tr('messageEditConfirm')),
          const SizedBox(height: 12),
          TextField(
            controller: _text,
            enabled: !_busy && !widget.isPending(),
            minLines: 5,
            maxLines: 16,
            onChanged: widget.onChanged,
          ),
          const SizedBox(height: 12),
          FilledButton(
            onPressed: _busy ? null : _submit(widget.isPending()),
            child: Text(
              context.tr(
                widget.isPending() ? 'messageCheck' : 'messageRegenerate',
              ),
            ),
          ),
        ],
      ),
    ),
  );
}
