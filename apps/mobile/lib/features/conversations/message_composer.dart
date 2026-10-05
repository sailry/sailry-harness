import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';

class MessageComposer extends StatelessWidget {
  const MessageComposer({
    super.key,
    required this.controller,
    required this.attached,
    required this.busy,
    required this.onAttachment,
    required this.onVoice,
    required this.onStop,
    required this.onSend,
    this.focusNode,
    this.priority,
    this.attachments,
    this.enabled = true,
    this.sendEnabled = true,
    this.attachmentsEnabled = true,
    this.voiceEnabled = true,
  });

  final TextEditingController controller;
  final FocusNode? focusNode;
  final bool attached;
  final bool busy;
  final ValueChanged<bool> onAttachment;
  final VoidCallback onVoice;
  final VoidCallback onStop;
  final VoidCallback onSend;
  final Widget? priority;
  final Widget? attachments;
  final bool enabled;
  final bool sendEnabled;
  final bool attachmentsEnabled;
  final bool voiceEnabled;

  @override
  Widget build(BuildContext context) {
    Widget icon(String name, String tooltip, VoidCallback? onPressed) =>
        SizedBox.square(
          dimension: 40,
          child: IconButton(
            padding: EdgeInsets.zero,
            tooltip: context.tr(tooltip),
            onPressed: onPressed,
            icon: AppIcon(name, size: 21),
          ),
        );
    return Padding(
      padding: EdgeInsets.fromLTRB(
        12,
        6,
        12,
        MediaQuery.paddingOf(context).bottom + 6,
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          ?priority,
          ?attachments,
          Surface(
            key: const ValueKey('composer-input'),
            radius: 28,
            padding: const EdgeInsets.all(6),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (attached)
                  Align(
                    alignment: Alignment.centerLeft,
                    child: InputChip(
                      label: Text(context.tr('attachment')),
                      avatar: const AppIcon('file', size: 15),
                      deleteIcon: const AppIcon('close', size: 18),
                      onDeleted: enabled ? () => onAttachment(false) : null,
                    ),
                  ),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.end,
                  children: [
                    icon(
                      'plus',
                      'attach',
                      enabled && attachmentsEnabled
                          ? () => onAttachment(true)
                          : null,
                    ),
                    Expanded(
                      child: TextField(
                        controller: controller,
                        focusNode: focusNode,
                        enabled: enabled,
                        minLines: 1,
                        maxLines: 6,
                        style: const TextStyle(fontSize: 15, height: 1.5),
                        textInputAction: TextInputAction.newline,
                        decoration: InputDecoration(
                          hintText: context.tr('describeTask'),
                          hintMaxLines: 1,
                          border: InputBorder.none,
                          enabledBorder: InputBorder.none,
                          focusedBorder: InputBorder.none,
                          filled: false,
                          isDense: true,
                          contentPadding: const EdgeInsets.symmetric(
                            horizontal: 4,
                            vertical: 9,
                          ),
                        ),
                      ),
                    ),
                    icon(
                      'mic',
                      'voice',
                      enabled && voiceEnabled ? onVoice : null,
                    ),
                    if (busy) icon('stop', 'stop', onStop),
                    SizedBox.square(
                      dimension: 40,
                      child: Center(
                        child: RoundButton(
                          icon: busy ? 'clock' : 'send',
                          primary: true,
                          tooltip: context.tr(busy ? 'enqueue' : 'send'),
                          onPressed: enabled && sendEnabled ? onSend : null,
                        ),
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
