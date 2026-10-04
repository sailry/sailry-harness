import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../l10n/conversations.dart';
import '../../ui/kit.dart';
import 'user_message.dart';

/// A finite set of presentation fixtures; this is not a session event reducer.
enum DemoPhase {
  thinking,
  reading,
  question,
  editing,
  approval,
  testing,
  reply,
  followup,
  complete,
  failed,
}

extension DemoPhaseLabel on DemoPhase {
  String get label => tr('phase${name[0].toUpperCase()}${name.substring(1)}');
  int get position => this == DemoPhase.failed ? 5 : index;
  String? get streamKey => switch (this) {
    DemoPhase.thinking => 'thoughtLive',
    DemoPhase.reading => 'readResult',
    DemoPhase.editing => 'editResult',
    DemoPhase.testing => 'testProgress',
    DemoPhase.reply => 'flowResult',
    DemoPhase.followup => 'followupThinking',
    _ => null,
  };
}

class ConversationTimeline extends StatelessWidget {
  const ConversationTimeline({
    super.key,
    required this.phase,
    required this.chars,
    required this.playing,
    required this.answered,
    required this.wideButton,
    required this.denied,
    required this.stopped,
    required this.failures,
    required this.followups,
    required this.onChanges,
    required this.onQuestion,
    required this.onRetry,
  });

  final DemoPhase phase;
  final int chars;
  final bool playing;
  final bool answered;
  final bool wideButton;
  final bool denied;
  final bool stopped;
  final int failures;
  final List<String> followups;
  final VoidCallback onChanges;
  final VoidCallback onQuestion;
  final VoidCallback onRetry;

  String _text(String key, bool active) {
    final text = tr(key);
    if (!active) return text;
    return text.substring(0, chars.clamp(0, text.length));
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final position = phase.position;
    final active = playing && !stopped && !denied;
    Widget prose(String key) => Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Text(
        tr(key),
        style: TextStyle(color: colors.onSurfaceVariant, height: 1.65),
      ),
    );
    Widget thought(String key, bool live) => TimelineDisclosure(
      key: ValueKey(key),
      icon: 'spark',
      label: tr(live ? 'thinkingNow' : 'thought'),
      initiallyExpanded: live,
      active: live && active,
      child: Text(
        _text(key, live),
        style: TextStyle(color: colors.onSurfaceVariant, height: 1.6),
      ),
    );
    Widget tool(
      String icon,
      String label,
      String target,
      String result, {
      bool live = false,
      bool failed = false,
      Widget? children,
    }) => TimelineDisclosure(
      key: ValueKey('$target-$result'),
      icon: icon,
      label: tr(label),
      detail: target,
      active: live && active,
      failed: failed,
      initiallyExpanded: live || failed,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ?children,
          Text(
            _text(result, live),
            style: TextStyle(
              color: failed ? colors.secondary : colors.onSurfaceVariant,
              height: 1.6,
            ),
          ),
        ],
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        TimelineDisclosure(
          key: const ValueKey('work-main'),
          icon: 'task',
          label: tr('workProcess'),
          detail: tr('workSteps').replaceAll(
            '{count}',
            '${(position >= 1 ? 3 : 0) + (position >= 3 ? 1 : 0) + (position >= 5 && !denied ? 1 : 0) + failures}',
          ),
          initiallyExpanded: position < 6,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              thought('thoughtLive', phase == DemoPhase.thinking),
              if (position >= 1)
                tool(
                  'file',
                  'toolReadLabel',
                  tr('readGroup'),
                  'readResult',
                  live: phase == DemoPhase.reading,
                  children: Column(
                    children: [
                      for (final path in [
                        'src/pages/Login.tsx',
                        'src/styles/forms.css',
                        'package.json',
                      ])
                        TimelineDisclosure(
                          icon: 'file',
                          label: tr('toolReadLabel'),
                          detail: path,
                          child: Text(tr('readFileResult')),
                        ),
                    ],
                  ),
                ),
              if (position >= 2) ...[
                prose('layoutFindings'),
                TimelineDisclosure(
                  icon: 'chat',
                  label: tr('questionRecord'),
                  detail: tr(answered ? 'answerRecorded' : 'questionPending'),
                  initiallyExpanded: !answered,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(tr('layoutQuestion')),
                      const SizedBox(height: 8),
                      if (answered)
                        Text(tr(wideButton ? 'wideButton' : 'keepButton'))
                      else
                        TextButton(
                          onPressed: onQuestion,
                          child: Text(tr('reply')),
                        ),
                    ],
                  ),
                ),
              ],
              if (position >= 3) ...[
                prose(wideButton ? 'editPlan' : 'editPlanKeep'),
                thought('editThinking', false),
                tool(
                  'edit',
                  'toolEditLabel',
                  'src/styles/login.css',
                  'editResult',
                  live: phase == DemoPhase.editing,
                ),
              ],
              if (position >= 4) ...[
                prose('beforeTest'),
                InkWell(
                  onTap: onChanges,
                  borderRadius: BorderRadius.circular(8),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 10),
                    child: Row(
                      children: [
                        AppIcon(
                          'code',
                          size: 16,
                          color: colors.onSurfaceVariant,
                        ),
                        const SizedBox(width: 7),
                        Expanded(
                          child: Text(
                            tr('changedFiles'),
                            style: TextStyle(color: colors.onSurfaceVariant),
                          ),
                        ),
                        Text('+42', style: TextStyle(color: colors.tertiary)),
                        const SizedBox(width: 4),
                        Text('−18', style: TextStyle(color: colors.secondary)),
                        const SizedBox(width: 4),
                        const AppIcon('chevron', size: 14),
                      ],
                    ),
                  ),
                ),
                TimelineDisclosure(
                  icon: 'shield',
                  label: 'pnpm test',
                  detail: tr(
                    denied
                        ? 'denied'
                        : stopped
                        ? 'stoppedStatus'
                        : position > 4
                        ? 'approved'
                        : 'awaiting',
                  ),
                  child: Text(tr('approvalBody')),
                ),
              ],
              for (var attempt = 0; attempt < failures; attempt++)
                tool(
                  'terminal',
                  'toolRunLabel',
                  'pnpm test',
                  'testFailure',
                  failed: true,
                ),
              if (position >= 5 && !denied)
                tool(
                  'terminal',
                  'toolRunLabel',
                  'pnpm test',
                  phase == DemoPhase.failed
                      ? 'testFailure'
                      : position == 5
                      ? 'testProgress'
                      : 'testResult',
                  live: phase == DemoPhase.testing,
                  failed: phase == DemoPhase.failed,
                ),
            ],
          ),
        ),
        if (phase == DemoPhase.failed)
          Align(
            alignment: Alignment.centerLeft,
            child: TextButton.icon(
              onPressed: onRetry,
              icon: const AppIcon('refresh', size: 16),
              label: Text(conversationTr('retryTask')),
            ),
          ),
        if (position >= 6 && !denied) ...[
          const SizedBox(height: 18),
          SelectableText(
            _text('flowResult', phase == DemoPhase.reply),
            style: const TextStyle(height: 1.7),
          ),
        ],
        if (position >= 7 && followups.isNotEmpty) ...[
          for (final text in followups) UserBubble(text: text),
          TimelineDisclosure(
            icon: 'task',
            label: tr('workProcess'),
            initiallyExpanded: phase == DemoPhase.followup,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                thought('followupThinking', phase == DemoPhase.followup),
                if (position >= 8)
                  tool(
                    'file',
                    'toolReadLabel',
                    'src/styles/login.css',
                    'followupToolResult',
                  ),
              ],
            ),
          ),
          if (position >= 8)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 16),
              child: Text(
                tr('followupResult'),
                style: const TextStyle(height: 1.7),
              ),
            ),
        ],
        if (stopped || denied || position >= 8)
          Padding(
            padding: const EdgeInsets.only(top: 18),
            child: Text(
              tr(
                stopped
                    ? 'stoppedStatus'
                    : denied
                    ? 'deniedResult'
                    : 'completed',
              ),
              style: TextStyle(color: colors.onSurfaceVariant),
            ),
          ),
      ],
    );
  }
}

class TimelineDisclosure extends StatelessWidget {
  const TimelineDisclosure({
    super.key,
    required this.icon,
    required this.label,
    required this.child,
    this.detail,
    this.initiallyExpanded = false,
    this.active = false,
    this.failed = false,
  });

  final String icon;
  final String label;
  final String? detail;
  final Widget child;
  final bool initiallyExpanded;
  final bool active;
  final bool failed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final color = failed ? colors.secondary : colors.onSurfaceVariant;
    return ExpansionTile(
      trailing: const DisclosureIcon(),
      tilePadding: EdgeInsets.zero,
      minTileHeight: 36,
      dense: true,
      shape: const Border(),
      collapsedShape: const Border(),
      initiallyExpanded: initiallyExpanded,
      maintainState: true,
      visualDensity: VisualDensity.compact,
      childrenPadding: const EdgeInsets.only(left: 7, bottom: 6),
      iconColor: color,
      collapsedIconColor: color,
      title: Row(
        children: [
          if (active)
            SizedBox(
              width: 15,
              height: 15,
              child: CircularProgressIndicator(
                strokeWidth: 1.5,
                color: colors.onSurfaceVariant,
              ),
            )
          else
            AppIcon(icon, size: 16, color: color),
          const SizedBox(width: 7),
          Text(label, style: TextStyle(fontSize: 14, color: color)),
          if (detail != null) ...[
            const SizedBox(width: 7),
            Expanded(
              child: Text(
                detail!,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontSize: 14, color: color),
              ),
            ),
          ],
        ],
      ),
      children: [
        Container(
          width: double.infinity,
          padding: const EdgeInsets.only(left: 13, top: 3, bottom: 3),
          decoration: BoxDecoration(
            border: Border(left: BorderSide(color: colors.outlineVariant)),
          ),
          child: child,
        ),
      ],
    );
  }
}
