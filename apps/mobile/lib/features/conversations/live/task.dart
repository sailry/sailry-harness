import 'dart:convert';

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../task_row.dart';
import 'page.dart';
import 'presentation.dart';
import 'recency.dart';

/// A one-line presentation of committed messages; tool output stays in the turn.
String messagePreview(
  List<Map<String, dynamic>> entries, {
  Translator translate = tr,
}) {
  for (final entry in entries.reversed) {
    final parts = objects(entry['parts']);
    final content = StringBuffer();
    var length = 0;
    var space = false;
    // A list row needs only a short prefix, even when a reply contains a large
    // document. Preserve part boundaries and Unicode without joining the reply.
    for (final part in parts) {
      if (part['kind'] != 'text') continue;
      for (final rune in text(part['data']).runes) {
        if (_previewWhitespace.hasMatch(String.fromCharCode(rune))) {
          space = length > 0;
          continue;
        }
        if (space) {
          content.write(' ');
          length++;
          space = false;
        }
        if (length >= 512) return content.toString().trimRight();
        content.writeCharCode(rune);
        if (++length == 512) return content.toString();
      }
    }
    if (content.isNotEmpty) return content.toString();
    for (final part in parts) {
      if (part['kind'] == 'attachment') {
        return text(
          object(object(part['data'])['spec'])['name'],
          translate('conversationAttachment'),
        );
      }
    }
  }
  return '';
}

final _previewWhitespace = RegExp(r'\s');

class ConversationTask extends StatefulWidget {
  const ConversationTask({
    super.key,
    required this.host,
    required this.session,
    required this.lane,
    required this.project,
    required this.now,
    required this.admitPreview,
    this.divider = true,
  });
  final HostConnection host;
  final Map<String, dynamic> session;
  final String lane;
  final Map<String, dynamic> project;

  /// Shared page clock so row times and list sections agree.
  final DateTime now;
  final bool divider;

  final bool Function() admitPreview;

  @override
  State<ConversationTask> createState() => _ConversationTaskState();
}

class _ConversationTaskState extends State<ConversationTask> {
  String? _stamp;
  String? _preview;
  bool _failed = false;
  int _request = 0;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void didUpdateWidget(ConversationTask oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.host != widget.host ||
        oldWidget.session['id'] != widget.session['id']) {
      _stamp = null;
      _preview = null;
    }
    _refresh();
  }

  void _refresh() {
    // Read markers, titles and configuration revisions do not change history.
    // Run transitions and queue/wait changes still refresh the committed preview.
    final activity = object(widget.session['activity']);
    final run = object(activity['run']);
    final committed = jsonEncode([
      run['turn'],
      run['sequence'],
      run['revision'],
      run['status'],
      run['finished_ms'],
      activity['waiting'],
      activity['queued'],
    ]);
    // The host's cached preview shows at once; a read refreshes it only when
    // the conversation moved on since it was taken.
    final cached = widget.host.preview(text(widget.session['id']));
    if (_preview == null && cached != null) _preview = cached.text;
    final stamp = '${widget.host.connected}$committed';
    if (stamp == _stamp || !widget.admitPreview()) return;
    _stamp = stamp;
    final request = ++_request;
    _failed = false;
    if (!widget.host.connected || cached?.stamp == committed) return;
    _read(request, committed);
  }

  Future<void> _read(int request, String committed) async {
    try {
      final host = widget.host;
      final session = widget.session['id'];
      final result = await host.command('read_conversation', {
        'session': session,
        'before': null,
        'limit': 1,
      });
      final history = object(result['data']);
      if (!mounted || request != _request) return;
      final page = object(history['page']);
      final entries = objects(page['entries']);
      var preview = messagePreview(entries, translate: context.tr);
      // Large tool-heavy turns may put the last message in an earlier chunk.
      // Follow the Node's read cursor only until that message is available.
      if (preview.isEmpty && (history['missing'] as List? ?? []).isNotEmpty) {
        final turn = (history['missing'] as List).last;
        var before = entries
            .where((entry) => entry['turn'] == turn)
            .firstOrNull?['sequence'];
        while (mounted && request == _request) {
          if (!widget.admitPreview()) {
            _stamp = null;
            return;
          }
          final result = await host.command('read_turn', {
            'session': session,
            'turn': turn,
            'expected_revision': page['revision'],
            'before': before,
            'limit': 100,
          });
          final chunk = object(result['data']);
          if (!mounted || request != _request) return;
          preview = messagePreview(
            objects(chunk['entries']),
            translate: context.tr,
          );
          final next = chunk['next_before'];
          if (preview.isNotEmpty || next == null) break;
          if (before != null && number(next) >= number(before)) {
            throw const FormatException('message preview did not advance');
          }
          before = next;
        }
      }
      host.keepPreview(text(session), committed, preview);
      if (mounted && request == _request) {
        _preview = preview;
        if (widget.admitPreview()) setState(() {});
      }
    } catch (error) {
      debugPrint('Conversation preview unavailable: $error');
      if (mounted && request == _request) {
        _failed = true;
        if (widget.admitPreview()) setState(() {});
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final connected = widget.host.connected;
    final session = widget.session;
    final tone = connected ? listTone(widget.lane) : null;
    final time = runTime(session);
    return TaskRow(
      icon: ProjectIcon(project: widget.project, size: 26),
      title: title(session, translate: context.tr),
      preview: _preview?.isNotEmpty == true
          ? _preview!
          : _failed
          ? context.tr('conversationPreviewUnavailable')
          : _preview != null
          ? context.tr('conversationNoMessages')
          : connected
          ? ''
          : context.tr('conversationOffline'),
      time: time == null
          ? null
          : timeLabel(
              time,
              widget.now,
              locale: dateLocale(Localizations.localeOf(context)),
              yesterday: context.tr('yesterday'),
            ),
      status: laneLabel(widget.lane, translate: context.tr),
      tone: tone,
      dim:
          tone == null &&
          object(object(session['activity'])['attention'])['unread'] != true,
      divider: widget.divider,
      onTap: () => pushPage(
        context,
        LiveConversationPage(
          host: widget.host,
          sessionId: widget.session['id'] as String,
          initialSession: widget.session,
        ),
      ),
    );
  }
}
