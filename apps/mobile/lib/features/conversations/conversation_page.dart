import 'dart:async';

import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
import '../resources/git_page.dart';
import '../resources/resources_page.dart';
import '../terminal/terminal_page.dart';
import 'conversation_timeline.dart';
import 'conversation_frame.dart';
import 'queue_sheet.dart';
import 'message_composer.dart';
import 'user_message.dart';

class ConversationPage extends StatefulWidget {
  const ConversationPage({
    super.key,
    this.title = '',
    this.project = 'sailry-web',
    this.host = 'Studio',
    this.branch = 'feature/sign-in',
    this.sample = true,
    this.question = false,
  });

  final String title;
  final String project;
  final String host;
  final String branch;
  final bool sample;
  final bool question;

  @override
  State<ConversationPage> createState() => _ConversationPageState();
}

class _ConversationPageState extends State<ConversationPage> {
  final _draft = TextEditingController();
  final _scroll = ScrollController();
  final List<String> _messages = [];
  final List<String> _followups = [];
  final List<QueuedDraft> _queue = [];
  bool _sampleLoaded = false;
  DemoPhase _phase = DemoPhase.approval;
  Timer? _timer;
  var _chars = 8;
  var _nextQueueId = 2;
  var _failures = 0;
  var _playing = false;
  var _stopped = false;
  var _denied = false;
  var _answered = true;
  var _wideButton = true;
  var _queuePaused = false;
  var _attached = false;
  var _model = 'Sonnet';

  bool get _busy =>
      widget.sample &&
      !_stopped &&
      !_denied &&
      _phase != DemoPhase.failed &&
      _phase != DemoPhase.complete;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (!_sampleLoaded) {
      _sampleLoaded = true;
      if (widget.sample) _queue.add(QueuedDraft(1, context.tr('queueSample')));
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    _draft.dispose();
    _scroll.dispose();
    super.dispose();
  }

  void _notify(String key) => showToast(context, context.tr(key));

  void _play() {
    _timer?.cancel();
    if (_phase.streamKey == null) return;
    setState(() {
      _playing = true;
      _stopped = false;
    });
    _timer = Timer.periodic(const Duration(milliseconds: 90), (_) {
      if (!mounted || !_playing || !_busy || _phase.streamKey == null) return;
      final atBottom = _scroll.hasClients && _scroll.position.extentAfter < 40;
      setState(() => _chars += 2);
      if (_chars > context.tr(_phase.streamKey!).length + 12) _next();
      if (atBottom) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted && _scroll.hasClients) {
            _scroll.jumpTo(_scroll.position.maxScrollExtent);
          }
        });
      }
    });
  }

  void _pause() {
    _timer?.cancel();
    setState(() => _playing = false);
  }

  void _next() {
    if ([
      _phase == DemoPhase.question,
      _phase == DemoPhase.approval,
      _phase == DemoPhase.failed,
      _phase == DemoPhase.complete,
      _denied,
      _stopped,
    ].any((value) => value)) {
      return;
    }
    setState(() {
      if (_phase == DemoPhase.reply) {
        if (_queue.isNotEmpty && !_queuePaused) {
          _followups.add(_queue.removeAt(0).text);
          _phase = DemoPhase.followup;
        } else {
          _phase = DemoPhase.complete;
        }
      } else {
        _phase = DemoPhase.values[_phase.index + 1];
      }
      _chars = 8;
    });
    if (_phase.streamKey == null) _pause();
  }

  void _choose(DemoPhase phase) {
    _timer?.cancel();
    setState(() {
      _phase = phase;
      _chars = 8;
      _playing = false;
      _stopped = false;
      _denied = false;
      _failures = 0;
      _answered = phase.position > 2;
      _followups.clear();
      _queue.clear();
      if (phase.position >= 3 && phase.position < 7) {
        _queue.add(QueuedDraft(_nextQueueId++, context.tr('queueSample')));
      } else if (phase.position >= 7) {
        _followups.add(context.tr('queueSample'));
      }
    });
  }

  void _approve(bool allowed) {
    setState(() {
      if (allowed) {
        _phase = DemoPhase.testing;
        _chars = 8;
      } else {
        _denied = true;
        _playing = false;
      }
    });
    if (allowed) {
      _play();
    } else {
      _timer?.cancel();
    }
  }

  void _question() {
    showAppSheet(
      context,
      context.tr('question'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(context.tr(widget.question ? 'question' : 'layoutQuestion')),
          const SizedBox(height: 16),
          for (final key
              in widget.question
                  ? ['optionChinese', 'optionEnglish']
                  : ['wideButton', 'keepButton'])
            ListTile(
              title: Text(context.tr(key)),
              trailing: const AppIcon('chevron'),
              onTap: () {
                Navigator.pop(context);
                setState(() {
                  _answered = true;
                  _wideButton = key == 'wideButton';
                  if (widget.sample) {
                    _phase = DemoPhase.editing;
                    _chars = 8;
                    if (_queue.isEmpty) {
                      _queue.add(
                        QueuedDraft(_nextQueueId++, context.tr('queueSample')),
                      );
                    }
                  } else {
                    _messages.add(context.tr(key));
                  }
                });
                if (widget.sample) _play();
              },
            ),
        ],
      ),
    );
  }

  void _queueSheet() {
    showAppSheet(
      context,
      context.tr('queue'),
      child: QueueSheet(
        items: _queue,
        paused: _queuePaused,
        busy: _busy,
        onChanged: () => setState(() {}),
        onPause: (value) => setState(() => _queuePaused = value),
        onSend: _sendNext,
      ),
    );
  }

  void _sendNext() {
    if (_queue.isEmpty || _busy) return;
    setState(() {
      _followups.add(_queue.removeAt(0).text);
      _phase = DemoPhase.followup;
      _chars = 8;
      _stopped = false;
      _denied = false;
    });
    _play();
  }

  void _retry() {
    setState(() {
      _failures++;
      _phase = DemoPhase.testing;
      _chars = 8;
      _stopped = false;
    });
    _play();
  }

  void _send() {
    final text = _draft.text.trim();
    if (text.isEmpty) return;
    setState(() {
      if (_busy) {
        _queue.add(QueuedDraft(_nextQueueId++, text));
      } else {
        _messages.add(text);
      }
      _draft.clear();
      _attached = false;
    });
    if (!_busy) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && _scroll.hasClients) {
          _scroll.animateTo(
            _scroll.position.maxScrollExtent,
            duration: const Duration(milliseconds: 200),
            curve: Curves.easeOut,
          );
        }
      });
    }
  }

  void _changes() => pushPage(
    context,
    GitPage(host: widget.host, project: widget.project, branch: widget.branch),
  );

  void _demo() {
    showAppSheet(
      context,
      context.tr('replyPreview'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            children: [
              Expanded(
                child: FilledButton(
                  onPressed: () {
                    Navigator.pop(context);
                    if (_phase == DemoPhase.complete || _denied) {
                      _choose(DemoPhase.thinking);
                    }
                    _play();
                  },
                  child: Text(context.tr('playFlow')),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: OutlinedButton(
                  onPressed: () {
                    _pause();
                    Navigator.pop(context);
                  },
                  child: Text(context.tr('pauseFlow')),
                ),
              ),
              IconButton(
                tooltip: context.tr('nextFlow'),
                onPressed: () {
                  _next();
                  Navigator.pop(context);
                },
                icon: const AppIcon('chevron'),
              ),
            ],
          ),
          const SizedBox(height: 12),
          for (final phase in DemoPhase.values)
            ListTile(
              title: Text(context.tr(phase.labelKey)),
              trailing: phase == _phase ? const AppIcon('check') : null,
              onTap: () {
                _choose(phase);
                Navigator.pop(context);
              },
            ),
        ],
      ),
    );
  }

  void _menu() {
    showAppSheet(
      context,
      context.tr('sessionActions'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          ListTile(
            leading: const AppIcon('terminal'),
            title: Text(context.tr('terminal')),
            trailing: const AppIcon('chevron'),
            onTap: () {
              Navigator.pop(context);
              pushPage(context, TerminalPage(project: widget.project));
            },
          ),
          ListTile(
            leading: const AppIcon('clock'),
            title: Text(context.tr('queue')),
            trailing: Text('${_queue.length}'),
            onTap: () {
              Navigator.pop(context);
              _queueSheet();
            },
          ),
          if (widget.sample)
            ListTile(
              leading: const AppIcon('spark'),
              title: Text(context.tr('replyPreview')),
              trailing: const AppIcon('chevron'),
              onTap: () {
                Navigator.pop(context);
                _demo();
              },
            ),
          ListTile(
            leading: const AppIcon('branch'),
            title: Text(context.tr('fork')),
            onTap: () {
              Navigator.pop(context);
              pushPage(
                context,
                ConversationPage(
                  title: widget.title,
                  project: widget.project,
                  host: widget.host,
                  branch: widget.branch,
                  sample: false,
                ),
              );
            },
          ),
          if (_busy)
            ListTile(
              leading: const AppIcon('stop'),
              title: Text(context.tr('stop')),
              onTap: () {
                Navigator.pop(context);
                _pause();
                setState(() => _stopped = true);
              },
            ),
        ],
      ),
    );
  }

  void _todos() {
    showAppSheet(
      context,
      context.tr('todoShort'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final (index, key)
              in (_phase == DemoPhase.followup
                      ? ['todoNarrow']
                      : ['todoInspect', 'todoEdit', 'todoTest'])
                  .indexed)
            ListTile(
              leading: AppIcon(
                _phase != DemoPhase.followup &&
                        _phase.position >= [3, 4, 6][index]
                    ? 'check'
                    : 'clock',
              ),
              title: Text(context.tr(key)),
              trailing: Text(
                context.tr(
                  _phase != DemoPhase.followup &&
                          _phase.position >= [3, 4, 6][index]
                      ? 'completed'
                      : 'toolPending',
                ),
              ),
            ),
        ],
      ),
    );
  }

  Widget? _priority(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    Widget main(
      String icon,
      String title,
      VoidCallback onTap, {
      String? subtitle,
    }) => Expanded(
      child: InkWell(
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 5),
          child: Row(
            children: [
              AppIcon(icon, size: 17, color: colors.secondary),
              const SizedBox(width: 7),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      title,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 14),
                    ),
                    if (subtitle != null)
                      Text(
                        subtitle,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 14,
                          color: colors.onSurfaceVariant,
                        ),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
    Widget? content;
    if (_busy && _phase == DemoPhase.approval) {
      content = Row(
        children: [
          main(
            'shield',
            'pnpm test',
            () => showAppSheet(
              context,
              context.tr('approval'),
              child: Text(context.tr('approvalBody')),
            ),
          ),
          TextButton(
            onPressed: () => _approve(false),
            child: Text(context.tr('deny')),
          ),
          FilledButton(
            onPressed: () => _approve(true),
            child: Text(context.tr('allowShort')),
          ),
        ],
      );
    } else if ((_busy && _phase == DemoPhase.question) ||
        (widget.question && _messages.isEmpty)) {
      content = Row(
        children: [
          main('chat', context.tr('confirmShort'), _question),
          FilledButton(onPressed: _question, child: Text(context.tr('reply'))),
        ],
      );
    } else if (_queue.isNotEmpty) {
      content = Row(
        children: [
          main(
            'clock',
            '${context.tr('queueShort')}  ${_queue.length}',
            _queueSheet,
          ),
          const AppIcon('chevron', size: 14),
        ],
      );
    } else if (widget.sample &&
        !_stopped &&
        !_denied &&
        (_phase.position < 6 || _phase == DemoPhase.followup)) {
      final index = _phase.position < 3
          ? 0
          : _phase.position < 4
          ? 1
          : 2;
      content = Row(
        children: [
          main(
            'task',
            _phase == DemoPhase.followup
                ? '${context.tr('todoShort')}  0/1'
                : '${context.tr('todoShort')}  $index/3',
            _todos,
            subtitle: context.tr(
              _phase == DemoPhase.followup
                  ? 'todoNarrow'
                  : ['todoInspect', 'todoEdit', 'todoTest'][index],
            ),
          ),
          if (_phase == DemoPhase.failed)
            TextButton(onPressed: _retry, child: Text(context.tr('retryTask')))
          else
            const AppIcon('chevron', size: 14),
        ],
      );
    } else if (widget.sample && _phase.position >= 3) {
      content = Row(
        children: [
          main('branch', context.tr('changes'), _changes),
          Text('+42', style: TextStyle(color: colors.tertiary)),
          const SizedBox(width: 5),
          Text('−18', style: TextStyle(color: colors.secondary)),
          const SizedBox(width: 8),
          const AppIcon('chevron', size: 14),
        ],
      );
    }
    if (content == null) return null;
    final theme = Theme.of(context);
    final buttonStyle = ButtonStyle(
      minimumSize: const WidgetStatePropertyAll(Size(40, 32)),
      padding: const WidgetStatePropertyAll(
        EdgeInsets.symmetric(horizontal: 8, vertical: 5),
      ),
      visualDensity: VisualDensity.standard,
      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      textStyle: WidgetStatePropertyAll(
        theme.textTheme.labelLarge?.copyWith(fontSize: 14, height: 1.4),
      ),
    );
    return Padding(
      key: const ValueKey('composer-priority'),
      padding: const EdgeInsets.symmetric(horizontal: 14),
      child: Material(
        color: colors.surfaceContainer,
        clipBehavior: Clip.antiAlias,
        shape: RoundedRectangleBorder(
          side: BorderSide(color: colors.outlineVariant),
          borderRadius: const BorderRadius.vertical(top: Radius.circular(18)),
        ),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
          child: Theme(
            data: theme.copyWith(
              textButtonTheme: TextButtonThemeData(
                style: buttonStyle.merge(theme.textButtonTheme.style),
              ),
              filledButtonTheme: FilledButtonThemeData(
                style: buttonStyle
                    .merge(theme.filledButtonTheme.style)
                    .copyWith(
                      shape: WidgetStatePropertyAll(
                        RoundedRectangleBorder(
                          borderRadius: BorderRadius.circular(8),
                        ),
                      ),
                    ),
              ),
            ),
            child: content,
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final title = widget.title.isEmpty ? context.tr('chatTitle') : widget.title;
    return ConversationFrame(
      title: title,
      leading: Align(
        alignment: Alignment.centerLeft,
        child: RoundButton(
          icon: 'settings',
          tooltip: context.tr('modelPicker'),
          onPressed: () => showAppSheet(
            context,
            context.tr('modelPicker'),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                for (final value in ['Sonnet', 'GPT', 'DeepSeek'])
                  ListTile(
                    title: Text(value),
                    trailing: value == _model ? const AppIcon('check') : null,
                    onTap: () {
                      setState(() => _model = value);
                      Navigator.pop(context);
                    },
                  ),
              ],
            ),
          ),
        ),
      ),
      actions: [
        RoundButton(
          icon: 'more',
          tooltip: context.tr('more'),
          onPressed: _menu,
        ),
        RoundButton(
          icon: 'folder',
          tooltip: context.tr('resources'),
          onPressed: () => pushPage(
            context,
            ResourcesPage(
              host: widget.host,
              project: widget.project,
              branch: widget.branch,
            ),
          ),
        ),
      ],
      bodyBuilder: (context, bottomInset) => Column(
        children: [
          Expanded(
            child: ListView(
              controller: _scroll,
              padding: EdgeInsets.fromLTRB(
                20,
                ConversationFrame.contentInset(context),
                20,
                bottomInset + 8,
              ),
              children: [
                UserBubble(
                  text: widget.sample ? context.tr('userMessage') : title,
                  time: widget.sample ? '09:36' : null,
                  attachment: widget.sample
                      ? SentAttachment(
                          name: 'login-reference.md',
                          onPressed: () => showAppSheet(
                            context,
                            'login-reference.md',
                            child: Text(context.tr('flowAttachment')),
                          ),
                        )
                      : null,
                ),
                if (widget.sample)
                  ConversationTimeline(
                    key: ValueKey('timeline-${_phase == DemoPhase.thinking}'),
                    phase: _phase,
                    chars: _chars,
                    playing: _playing,
                    answered: _answered,
                    wideButton: _wideButton,
                    denied: _denied,
                    stopped: _stopped,
                    failures: _failures,
                    followups: _followups,
                    onChanges: _changes,
                    onQuestion: _question,
                    onRetry: _retry,
                  )
                else if (widget.question && _messages.isEmpty)
                  Text(
                    context.tr('question'),
                    style: const TextStyle(height: 1.7),
                  ),
                for (final message in _messages) UserBubble(text: message),
              ],
            ),
          ),
        ],
      ),
      composer: MessageComposer(
        controller: _draft,
        attached: _attached,
        busy: _busy,
        priority: _priority(context),
        onAttachment: (value) => setState(() => _attached = value),
        onVoice: () => _notify('voiceNote'),
        onStop: () {
          _pause();
          setState(() => _stopped = true);
        },
        onSend: _send,
      ),
    );
  }
}
