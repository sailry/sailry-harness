import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:sailry_bridge/api/terminal.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
import 'appearance.dart';
import 'cursor.dart';
import 'grid.dart';
import 'toolbar.dart';

class LiveTerminalPage extends StatefulWidget {
  const LiveTerminalPage({
    super.key,
    this.hostId,
    this.terminalId,
    this.worktreeId,
    this.title = '',
    this.createNew = false,
  });
  final String? hostId;
  final String? terminalId;
  final String? worktreeId;
  final String title;
  final bool createNew;
  @override
  State<LiveTerminalPage> createState() => _LiveTerminalPageState();
}

class _LiveTerminalPageState extends State<LiveTerminalPage>
    with WidgetsBindingObserver {
  static const _sentinel = '\u200b';
  final _draft = TextEditingController(text: _sentinel);
  final _focus = FocusNode();
  final _editor = GlobalKey<EditableTextState>();
  final _blink = CursorBlink();
  final _scroll = ScrollController();
  final _modifiers = <String>{};
  HostConnection? _host;
  TerminalUpdates? _updates;
  String? _terminal;
  String? _launchRequest;
  int? _lease;
  Map<String, dynamic> _snapshot = {};
  Map<String, dynamic>? _viewport;
  Map<String, dynamic>? _pendingViewport;
  Timer? _resizeTimer;
  String? _appearance;
  String? _error;
  bool _opening = false;
  bool _started = false;
  bool _connected = false;
  bool _resetting = false;
  bool _foreground = true;
  bool _visible = true;
  Future<void> _inputTail = Future.value();
  CellPosition? _anchor;
  CellPosition? _extent;

  Map<String, dynamic> get _info => object(_snapshot['info']);
  Map<String, dynamic> get _screen => object(_snapshot['screen']);
  bool get _running => object(_info['status'])['kind'] == 'running';
  bool get _controlling =>
      _connected && _running && _lease != null && _info['revision'] == _lease;
  bool get _cursorFocused =>
      _controlling && _focus.hasFocus && _foreground && _visible;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    final lifecycle = WidgetsBinding.instance.lifecycleState;
    _foreground = lifecycle == null || lifecycle == AppLifecycleState.resumed;
    _draft.addListener(_typed);
    _focus.addListener(_focused);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _visible = TickerMode.valuesOf(context).enabled;
    _syncCursor();
    final session = AppSession.of(context);
    _host ??= widget.hostId == null
        ? session.selectedHost
        : session.host(widget.hostId);
    if (!_started && _host?.connected == true) {
      _started = true;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _open();
      });
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _blink.dispose();
    _resizeTimer?.cancel();
    final updates = _updates;
    _updates = null;
    if (updates != null) {
      unawaited(updates.close().whenComplete(updates.dispose));
    }
    _draft.dispose();
    _focus.dispose();
    _scroll.dispose();
    super.dispose();
  }

  Future<void> _open() async {
    final host = _host;
    if (host == null || !host.connected || _opening) return;
    setState(() {
      _opening = true;
      _error = null;
    });
    try {
      String? terminal = _terminal ?? widget.terminalId;
      if (terminal == null &&
          !widget.createNew &&
          _launchRequest == null &&
          widget.worktreeId != null) {
        final existing = objects(host.snapshot['terminals'])
            .where(
              (info) =>
                  info['worktree'] == widget.worktreeId &&
                  object(info['status'])['kind'] != 'closed',
            )
            .firstOrNull;
        terminal = existing?['id'] as String?;
      }
      if (terminal == null) {
        if (widget.worktreeId == null) {
          throw StateError('A worktree is required to create a terminal');
        }
        final result = _launchRequest == null
            ? await host.command('create_terminal', {
                'worktree': widget.worktreeId,
                ...terminalLaunch(context),
              })
            : await host.execute(_launchRequest!);
        final info = object(result['data']);
        terminal = text(info['id']);
        _terminal = terminal;
        _launchRequest = null;
        _lease = number(info['revision']).toInt();
      } else {
        _terminal = terminal;
        // The Node alone decides whether the persisted terminal needs a new PTY.
        if (_launchRequest == null) {
          await host.command('open_terminal', {
            'terminal': terminal,
            ...terminalLaunch(context),
          });
        } else {
          await host.execute(_launchRequest!);
        }
        _launchRequest = null;
      }
      if (!mounted) return;
      _terminal = terminal;
      final updates = await host.connection.watchTerminal(terminal: terminal);
      if (!mounted) {
        await updates.close();
        updates.dispose();
        return;
      }
      _updates = updates;
      unawaited(_watch(updates));
    } on CommandFailure catch (error) {
      if (mounted) {
        setState(() {
          if (error.code == 'outcome_unknown') _launchRequest = error.request;
          _error = failureText(error);
        });
      }
    } catch (error) {
      if (mounted) setState(() => _error = failureText(error));
    } finally {
      if (mounted) setState(() => _opening = false);
    }
  }

  Future<void> _watch(TerminalUpdates updates) async {
    try {
      while (mounted && identical(_updates, updates)) {
        final view = object(jsonDecode(await updates.next()));
        if (!mounted || !identical(_updates, updates)) return;
        final atBottom =
            !_scroll.hasClients || _scroll.position.extentAfter < 2;
        final controlled = _controlling;
        final cursor = jsonEncode(_screen['cursor']);
        setState(() {
          _connected = view['connected'] == true;
          _snapshot = object(view['snapshot']);
          _error = view['error'] == null
              ? null
              : text(object(view['error'])['message']);
          if (!_connected ||
              _lease != null && number(_info['revision']) > _lease!) {
            _lease = null;
          }
        });
        _syncCursor(restart: cursor != jsonEncode(_screen['cursor']));
        if (controlled && !_controlling) {
          _cancelResize();
          _resetInput();
          _focus.unfocus();
        }
        if (atBottom) {
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (mounted && _scroll.hasClients) {
              _scroll.jumpTo(_scroll.position.maxScrollExtent);
            }
          });
        }
      }
    } catch (error) {
      if (mounted && identical(_updates, updates)) {
        _cancelResize();
        setState(() {
          _updates = null;
          _connected = false;
          _lease = null;
          _error = failureText(error);
        });
        _syncCursor();
        await updates.close();
        updates.dispose();
      }
    }
  }

  Future<void> _claim() async {
    if (!_connected || !_running || _opening) return;
    setState(() {
      _opening = true;
      _error = null;
    });
    try {
      final result = await _host!.command('claim_terminal', {
        'terminal': _terminal,
        'expected_revision': _info['revision'],
      });
      if (!mounted) return;
      _cancelResize();
      setState(() {
        _lease = number(object(result['data'])['revision']).toInt();
        _viewport = null;
        _appearance = null;
      });
      _syncCursor();
      _focus.requestFocus();
    } catch (error) {
      if (mounted) showToast(context, failureText(error));
    } finally {
      if (mounted) setState(() => _opening = false);
    }
  }

  void _enqueue(String kind, Map<String, dynamic> data) {
    if (!_controlling) return;
    final host = _host!;
    final terminal = _terminal;
    final revision = _lease;
    _inputTail = _inputTail
        .then((_) async {
          if (!mounted ||
              !_controlling ||
              _lease != revision ||
              _terminal != terminal) {
            return;
          }
          await host.command(kind, {
            'terminal': terminal,
            'revision': revision,
            ...data,
          });
        })
        .catchError((Object error) {
          // A failed input invalidates the queued tail. Terminal keystrokes are never replayed.
          if (mounted && _lease == revision && _terminal == terminal) {
            _cancelResize();
            setState(() {
              _lease = null;
            });
            showToast(context, failureText(error));
            _syncCursor();
            _resetInput();
            _focus.unfocus();
          }
        });
  }

  void _input(Map<String, dynamic> input) {
    if (!_controlling) return;
    _syncCursor(restart: true);
    setState(() {
      _anchor = null;
      _extent = null;
    });
    _enqueue('input_terminal', {'input': input});
  }

  Map<String, bool> get _keys => {
    'shift':
        _modifiers.contains('Shift') ||
        HardwareKeyboard.instance.isShiftPressed,
    'control':
        _modifiers.contains('Ctrl') ||
        HardwareKeyboard.instance.isControlPressed,
    'alt': _modifiers.contains('Alt') || HardwareKeyboard.instance.isAltPressed,
    'super_key':
        _modifiers.contains('Cmd') || HardwareKeyboard.instance.isMetaPressed,
    'caps_lock': false,
    'num_lock': false,
  };

  void _key(Object key, {String action = 'press', String? character}) =>
      _input({
        'key': {
          'event': {
            'key': key,
            'action': action,
            'modifiers': _keys,
            'utf8': character,
            'unshifted_codepoint': character?.runes.firstOrNull,
          },
        },
      });

  void _resetInput() {
    _resetting = true;
    _draft.value = const TextEditingValue(
      text: _sentinel,
      selection: TextSelection.collapsed(offset: 1),
    );
    _resetting = false;
  }

  void _typed() {
    if (!_resetting) _syncCursor(restart: true);
    if (_resetting ||
        _draft.value.composing.isValid && !_draft.value.composing.isCollapsed) {
      return;
    }
    final value = _draft.text;
    if (value == _sentinel) return;
    if (!_controlling) {
      _resetInput();
      return;
    }
    if (value.isEmpty) {
      _key('backspace');
    } else {
      final committed = value.startsWith(_sentinel)
          ? value.substring(1)
          : value;
      if (committed.isNotEmpty) {
        if (_keys.entries.any((entry) => entry.key != 'shift' && entry.value)) {
          for (final codepoint in committed.runes) {
            _key({
              'character': {'codepoint': codepoint},
            }, character: String.fromCharCode(codepoint));
          }
        } else if (committed.contains(RegExp(r'[\r\n\t]'))) {
          _input({
            'paste': {'text': committed},
          });
        } else {
          _input({
            'text': {'text': committed},
          });
        }
      }
    }
    _resetInput();
  }

  void _focused() {
    if (!_focus.hasFocus) _resetInput();
    _syncCursor(restart: true);
    setState(() {});
    _enqueue('input_terminal', {
      'input': {
        'focus': {'focused': _focus.hasFocus},
      },
    });
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    setState(() => _foreground = state == AppLifecycleState.resumed);
    _syncCursor(restart: true);
  }

  void _syncCursor({bool restart = false}) {
    final composing = _draft.value.composing;
    _blink.update(
      enabled:
          _cursorFocused &&
          _screen['cursor'] != null &&
          object(_screen['cursor'])['blinking'] == true &&
          !(composing.isValid && !composing.isCollapsed),
      restart: restart,
    );
  }

  KeyEventResult _hardware(FocusNode node, KeyEvent event) {
    if (!_controlling) return KeyEventResult.ignored;
    final key = {
      LogicalKeyboardKey.enter: 'enter',
      LogicalKeyboardKey.numpadEnter: 'enter',
      LogicalKeyboardKey.tab: 'tab',
      LogicalKeyboardKey.escape: 'escape',
      LogicalKeyboardKey.backspace: 'backspace',
      LogicalKeyboardKey.delete: 'delete',
      LogicalKeyboardKey.arrowUp: 'arrow_up',
      LogicalKeyboardKey.arrowDown: 'arrow_down',
      LogicalKeyboardKey.arrowLeft: 'arrow_left',
      LogicalKeyboardKey.arrowRight: 'arrow_right',
      LogicalKeyboardKey.home: 'home',
      LogicalKeyboardKey.end: 'end',
      LogicalKeyboardKey.pageUp: 'page_up',
      LogicalKeyboardKey.pageDown: 'page_down',
    }[event.logicalKey];
    final action = event is KeyUpEvent
        ? 'release'
        : event is KeyRepeatEvent
        ? 'repeat'
        : 'press';
    if (key != null) {
      _key(key, action: action);
      return KeyEventResult.handled;
    }
    if (HardwareKeyboard.instance.isControlPressed ||
        HardwareKeyboard.instance.isAltPressed ||
        HardwareKeyboard.instance.isMetaPressed) {
      final character = event.logicalKey.keyLabel.toLowerCase();
      if (character.runes.length == 1) {
        _key(
          {
            'character': {'codepoint': character.runes.single},
          },
          action: action,
          character: character,
        );
        return KeyEventResult.handled;
      }
    }
    return KeyEventResult.ignored;
  }

  Future<void> _paste() async {
    final clipboard = await Clipboard.getData(Clipboard.kTextPlain);
    if (!mounted || !_controlling) return;
    final value = clipboard?.text;
    if (value != null && value.isNotEmpty) {
      _input({
        'paste': {'text': value},
      });
    }
  }

  void _geometry(Size size, Size cell) {
    if (!_controlling) return;
    final viewport = terminalViewport(size, cell);
    final appearance = terminalAppearance(context);
    final encoded = jsonEncode(appearance);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted || !_controlling) return;
      _scheduleResize(viewport);
      if (_appearance != encoded) {
        _appearance = encoded;
        _enqueue('set_terminal_appearance', {'appearance': appearance});
      }
    });
  }

  void _cancelResize() {
    _resizeTimer?.cancel();
    _resizeTimer = null;
    _pendingViewport = null;
  }

  void _scheduleResize(Map<String, dynamic> viewport) {
    if (jsonEncode(_pendingViewport ?? _viewport) == jsonEncode(viewport)) {
      return;
    }
    _cancelResize();
    if (jsonEncode(_viewport) == jsonEncode(viewport)) return;
    _pendingViewport = viewport;
    final lease = _lease;
    // Keyboard animation reports intermediate heights. Resize the PTY only
    // after layout settles so its process redraws once, keeping the last grid.
    _resizeTimer = Timer(const Duration(milliseconds: 160), () {
      _resizeTimer = null;
      _pendingViewport = null;
      if (!mounted || !_controlling || _lease != lease) return;
      _viewport = viewport;
      _enqueue('resize_terminal', {'viewport': viewport});
    });
  }

  void _keyboard() {
    if (MediaQuery.viewInsetsOf(context).bottom > 0) {
      _focus.unfocus();
    } else {
      _editor.currentState?.requestKeyboard();
    }
  }

  @override
  Widget build(BuildContext context) {
    final screen = _screen;
    final lines = terminalLines(screen);
    final cell = terminalCell(context);
    final selected = _anchor == null || _extent == null
        ? ''
        : terminalSelection(lines, _anchor!, _extent!);
    return PageFrame(
      loading: _opening && _updates == null,
      title: widget.title.isEmpty ? tr('terminal') : widget.title,
      scroll: false,
      actions: [
        if (selected.isNotEmpty)
          RoundButton(
            icon: 'copy',
            tooltip: tr('copy'),
            onPressed: () => Clipboard.setData(ClipboardData(text: selected)),
          ),
        RoundButton(
          icon: 'paste',
          tooltip: tr('resourceTerminalPaste'),
          onPressed: _controlling ? _paste : null,
        ),
      ],
      child: Column(
        children: [
          if (_snapshot.isNotEmpty && !_running)
            Text(tr('resourceTerminalEnded')),
          Expanded(
            child:
                _error != null ||
                    _host?.connected != true ||
                    (!_connected && _snapshot.isNotEmpty)
                ? FailureState(
                    icon: 'terminal',
                    message: _error ?? tr('resourceDisconnected'),
                    onRetry:
                        _opening || _host?.connected != true || _updates != null
                        ? null
                        : _open,
                  )
                : screen.isEmpty
                ? Center(child: Text(tr('resourceTerminalConnecting')))
                : LayoutBuilder(
                    builder: (context, constraints) {
                      _geometry(constraints.biggest, cell);
                      final width = math.max(
                        constraints.maxWidth,
                        number(screen['columns']) * cell.width,
                      );
                      final height = math.max(
                        constraints.maxHeight,
                        lines.length * cell.height,
                      );
                      CellPosition hit(Offset position) => (
                        row: (position.dy / cell.height).floor().clamp(
                          0,
                          math.max(0, lines.length - 1),
                        ),
                        column: (position.dx / cell.width).floor().clamp(
                          0,
                          math.max(0, number(screen['columns']).toInt() - 1),
                        ),
                      );
                      return Stack(
                        children: [
                          Positioned.fill(
                            child: SingleChildScrollView(
                              scrollDirection: Axis.horizontal,
                              child: SizedBox(
                                width: width,
                                child: SingleChildScrollView(
                                  controller: _scroll,
                                  child: GestureDetector(
                                    behavior: HitTestBehavior.opaque,
                                    onTap: () {
                                      if (_controlling) {
                                        _editor.currentState?.requestKeyboard();
                                      }
                                    },
                                    onLongPressStart: (event) => setState(() {
                                      _anchor = hit(event.localPosition);
                                      _extent = _anchor;
                                    }),
                                    onLongPressMoveUpdate: (event) => setState(
                                      () => _extent = hit(event.localPosition),
                                    ),
                                    child: Semantics(
                                      label: tr('terminal'),
                                      child: CustomPaint(
                                        size: Size(width, height),
                                        painter: TerminalGridPainter(
                                          screen: screen,
                                          cell: cell,
                                          fontSize: terminalFontSize(context),
                                          anchor: _anchor,
                                          extent: _extent,
                                          focused: _cursorFocused,
                                          blink: _blink,
                                          selectionColor: Theme.of(context)
                                              .colorScheme
                                              .primary
                                              .withValues(alpha: .2),
                                        ),
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                            ),
                          ),
                          // EditableText owns the platform IME. Preedit stays here; only a committed
                          // value reaches the Node's existing text/key/paste encoder.
                          Positioned(
                            left: 0,
                            bottom: 0,
                            width: 1,
                            height: 1,
                            child: Opacity(
                              opacity: 0,
                              child: Focus(
                                onKeyEvent: _hardware,
                                child: EditableText(
                                  key: _editor,
                                  controller: _draft,
                                  focusNode: _focus,
                                  readOnly: !_controlling,
                                  style: TextStyle(
                                    fontFamily: 'monospace',
                                    fontSize: terminalFontSize(context),
                                  ),
                                  cursorColor: Theme.of(
                                    context,
                                  ).colorScheme.onSurface,
                                  backgroundCursorColor: Theme.of(
                                    context,
                                  ).colorScheme.surface,
                                  keyboardType: TextInputType.text,
                                  textInputAction: TextInputAction.send,
                                  autocorrect: false,
                                  enableSuggestions: false,
                                  onSubmitted: (_) {
                                    _key('enter');
                                    _focus.requestFocus();
                                  },
                                ),
                              ),
                            ),
                          ),
                          if (_connected && _running && !_controlling)
                            Positioned.fill(
                              child: Material(
                                color: Theme.of(
                                  context,
                                ).colorScheme.surface.withValues(alpha: .72),
                                child: InkWell(
                                  key: const ValueKey(
                                    'terminal-control-overlay',
                                  ),
                                  onTap: _opening ? null : _claim,
                                  child: Center(
                                    child: Text(
                                      tr(
                                        _opening
                                            ? 'resourceTerminalClaiming'
                                            : 'resourceTerminalControlHint',
                                      ),
                                      style: Theme.of(
                                        context,
                                      ).textTheme.bodyMedium,
                                    ),
                                  ),
                                ),
                              ),
                            ),
                        ],
                      );
                    },
                  ),
          ),
          TerminalToolbar(
            modifiers: _modifiers,
            keyboardVisible: MediaQuery.viewInsetsOf(context).bottom > 0,
            onKeyboard: _controlling ? _keyboard : null,
            onKey: _controlling ? _key : null,
            onModifier: _controlling
                ? (key) => setState(() {
                    if (!_modifiers.add(key)) _modifiers.remove(key);
                  })
                : null,
          ),
        ],
      ),
    );
  }
}
