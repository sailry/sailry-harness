import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
import '../../runtime/session.dart';
import 'live_terminal.dart';
import 'toolbar.dart';
export 'appearance.dart' show terminalLaunch;

/// The standalone preview is used only without a live application session.
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

  void _placeholder() => showToast(context, tr('terminalUnavailable'));

  @override
  Widget build(BuildContext context) {
    if (AppSession.maybeOf(context) != null) {
      return LiveTerminalPage(
        hostId: widget.hostId,
        terminalId: widget.terminalId,
        worktreeId: widget.worktreeId,
        title: widget.title,
        createNew: widget.createNew,
      );
    }
    final colors = Theme.of(context).colorScheme;
    final output = tr(
      'terminalFixture',
    ).replaceFirst('sailry-web', widget.project);
    return PageFrame(
      title: tr('terminal'),
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
