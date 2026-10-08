import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/toast.dart';
import 'package:sailry_mobile/features/terminal/toolbar.dart';
export 'package:sailry_mobile/features/terminal/appearance.dart'
    show terminalLaunch;

/// Presentation fixtures for the explicit preview entry point.
class TerminalPage extends StatefulWidget {
  const TerminalPage({
    super.key,
    this.hostId,
    this.terminalId,
    this.worktreeId,
    this.title = '',
    this.project = 'sailry-web',
    this.createNew = false,
  });
  final String title;
  final String? hostId;
  final String? terminalId;
  final String? worktreeId;
  final String project;
  final bool createNew;
  @override
  State<TerminalPage> createState() => _TerminalPageState();
}

class _TerminalPageState extends State<TerminalPage> {
  final _input = TextEditingController();
  final _focus = FocusNode();
  final _modifiers = <String>{};
  @override
  void dispose() {
    _input.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _placeholder() => showToast(context, context.tr('terminalUnavailable'));

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final output = context
        .tr('terminalFixture')
        .replaceFirst('sailry-web', widget.project);
    return PageFrame(
      title: context.tr('terminal'),
      scroll: false,
      child: Column(
        children: [
          Expanded(
            child: SingleChildScrollView(
              child: Align(
                alignment: Alignment.topLeft,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SelectableText(
                      output,
                      style: TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 14,
                        height: 1.75,
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                    TextField(
                      key: const ValueKey('terminal-draft'),
                      controller: _input,
                      focusNode: _focus,
                      style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 14,
                      ),
                      decoration: const InputDecoration(
                        filled: false,
                        border: InputBorder.none,
                        enabledBorder: InputBorder.none,
                        focusedBorder: InputBorder.none,
                        contentPadding: EdgeInsets.zero,
                      ),
                      onSubmitted: (_) => _placeholder(),
                    ),
                  ],
                ),
              ),
            ),
          ),
          TerminalToolbar(
            modifiers: _modifiers,
            keyboardVisible: MediaQuery.viewInsetsOf(context).bottom > 0,
            onKeyboard: () {
              if (MediaQuery.viewInsetsOf(context).bottom > 0) {
                _focus.unfocus();
              } else {
                _focus.requestFocus();
              }
            },
            onModifier: (key) => setState(() {
              if (!_modifiers.add(key)) _modifiers.remove(key);
            }),
            onKey: (key) {
              if (key == 'escape') {
                _focus.unfocus();
                setState(_modifiers.clear);
              } else if (key == 'tab') {
                _input.text += '  ';
              } else {
                _placeholder();
              }
            },
          ),
        ],
      ),
    );
  }
}
