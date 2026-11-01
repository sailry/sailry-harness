import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'choice_row.dart';
import 'team.dart';
import 'team_controls.dart';

/// Edits a configuration draft; the owning form admits the revision to the Node.
class ModelControls extends StatelessWidget {
  const ModelControls({
    super.key,
    required this.providers,
    required this.config,
    required this.onChanged,
    this.onCommitted,
    this.enabled = true,
    this.team,
  });

  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final ValueChanged<Map<String, dynamic>> onChanged;
  final ValueChanged<Map<String, dynamic>>? onCommitted;
  final bool enabled;

  /// A live session's team; new conversations use the Default team.
  final TeamChoices? team;

  void _commit(Map<String, dynamic> value) {
    onChanged(value);
    onCommitted?.call(value);
  }

  @override
  Widget build(BuildContext context) {
    final provider = providers
        .where((p) => p['id'] == config['provider'])
        .firstOrNull;
    final model = objects(
      provider?['models'],
    ).where((m) => m['id'] == config['model']).firstOrNull;
    final efforts = <String>{
      ...(model?['efforts'] as List? ?? []).whereType<String>(),
    }.toList();
    final initial = model?['default_effort'] ?? 'default';
    final current = config['effort'] ?? 'default';
    final selected = efforts.indexOf(current is String ? current : '');
    final budget = object(current)['budget'];
    final label = budget == null
        ? text(current)
        : '${context.tr('conversationBudget')}: $budget';
    final colors = Theme.of(context).colorScheme;
    final state = team?.read();
    final selectedTeam = state?.team;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            const SizedBox(width: 40),
            Expanded(
              child: Text(
                label,
                textAlign: TextAlign.center,
                style: Theme.of(
                  context,
                ).textTheme.titleMedium?.copyWith(color: colors.primary),
              ),
            ),
            IconButton(
              style: IconButton.styleFrom(
                minimumSize: const Size.square(40),
                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              ),
              tooltip: context.tr('resetReasoning'),
              onPressed: enabled && current != initial
                  ? () => _commit({...config, 'effort': initial})
                  : null,
              icon: const AppIcon('refresh', size: 20),
            ),
          ],
        ),
        TextButton(
          key: const ValueKey('choose-model'),
          style: TextButton.styleFrom(
            minimumSize: const Size(40, 36),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          onPressed: !enabled
              ? null
              : () async {
                  final selection = await showAppSheet<Map<String, dynamic>>(
                    context,
                    context.tr('modelPicker'),
                    scroll: false,
                    child: ModelChoices(
                      providers: providers,
                      config: config,
                      team: team,
                    ),
                  );
                  if (selection != null) {
                    final current = team?.read().config ?? config;
                    _commit({...current, ...selection});
                  }
                },
          child: _withTooltip(
            // A team chip hides the Lead model; the tooltip keeps it available.
            selectedTeam == null
                ? null
                : '${chipLabel(null, config, context.tr)} · $label',
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (selectedTeam != null) ...[
                  TeamIcon(team: selectedTeam),
                  const SizedBox(width: 6),
                ],
                Flexible(
                  child: Text(
                    chipLabel(state, config, context.tr),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                const SizedBox(width: 8),
                const AppIcon('chevron', size: 16),
              ],
            ),
          ),
        ),
        if (efforts.isNotEmpty) ...[
          SliderTheme(
            data: SliderTheme.of(context).copyWith(
              trackHeight: 18,
              thumbShape: const RoundSliderThumbShape(enabledThumbRadius: 13),
              overlayShape: const RoundSliderOverlayShape(overlayRadius: 22),
            ),
            child: Slider(
              key: const ValueKey('reasoning-slider'),
              value: selected < 0 ? 0 : selected.toDouble(),
              max: efforts.length > 1 ? (efforts.length - 1).toDouble() : 1,
              divisions: efforts.length > 1 ? efforts.length - 1 : 1,
              semanticFormatterCallback: (value) =>
                  efforts[value.round().clamp(0, efforts.length - 1)],
              onChanged: enabled && efforts.length > 1
                  ? (value) =>
                        onChanged({...config, 'effort': efforts[value.round()]})
                  : null,
              onChangeEnd: enabled && efforts.length > 1
                  ? (value) => onCommitted?.call({
                      ...config,
                      'effort': efforts[value.round()],
                    })
                  : null,
            ),
          ),
        ],
        const SizedBox(height: 12),
      ],
    );
  }
}

Widget _withTooltip(String? message, Widget child) =>
    message == null ? child : Tooltip(message: message, child: child);

String _modelLabel(Map<String, dynamic> model) =>
    text(model['display_name'], text(model['id']));

/// Providers sharing a preset form one family, like the Desktop rail.
String _family(Map<String, dynamic> provider) {
  final preset = text(object(provider['preset'])['id']);
  return preset.isEmpty ? text(provider['name']) : preset;
}

/// Desktop-style picker: a family rail beside collapsible provider groups.
/// With a team, the rail and list pick the model of the selected avatar.
class ModelChoices extends StatefulWidget {
  const ModelChoices({
    super.key,
    required this.providers,
    required this.config,
    this.team,
  });
  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final TeamChoices? team;

  @override
  State<ModelChoices> createState() => _ModelChoicesState();
}

class _ModelChoicesState extends State<ModelChoices> {
  late final List<Map<String, dynamic>> _providers = widget.providers
      .where((p) => p['enabled'] == true && objects(p['models']).isNotEmpty)
      .toList();
  late final _families = <String>{
    for (final p in _providers) _family(p),
  }.toList();
  String? _familyKey;
  (Object?, Object?)? _selectedModel;
  final _collapsed = <Object?>{};

  /// The selected member's role ID; null selects the Lead.
  String? _member;
  bool _choosingTeam = false;
  bool _busy = false;

  String? _familyOf(Object? provider) {
    final current = _providers.where((p) => p['id'] == provider).firstOrNull;
    return current == null ? _families.firstOrNull : _family(current);
  }

  Future<void> _run(Future<void> Function() action) async {
    if (_busy) return;
    setState(() => _busy = true);
    try {
      await action();
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  void _select(SessionTeam state, String? member) => setState(() {
    _member = member;
    final role = state.members.where((m) => m['id'] == member).firstOrNull;
    _familyKey = _familyOf(state.selection(role)['provider']);
  });

  @override
  Widget build(BuildContext context) {
    if (_providers.isEmpty) {
      return EmptyState(message: context.tr('conversationNoModel'));
    }
    final height = MediaQuery.sizeOf(context).height * .6;
    final team = widget.team;
    if (team == null) {
      return SizedBox(
        height: height,
        child: _models(context, selection: widget.config),
      );
    }
    return ListenableBuilder(
      listenable: team.listenable,
      builder: (context, _) {
        final state = team.read();
        final member = state.members
            .where((m) => m['id'] == _member)
            .firstOrNull;
        final model = member == null ? null : state.model(member);
        final role = text(member?['id']);
        return SizedBox(
          height: height,
          child: AbsorbPointer(
            absorbing: _busy,
            child: _choosingTeam
                ? TeamList(
                    state: state,
                    onBack: () => setState(() => _choosingTeam = false),
                    onSelected: (selected) => _run(() async {
                      await team.onTeam(selected);
                      if (mounted) {
                        setState(() {
                          _choosingTeam = false;
                          _member = null;
                          _familyKey = _familyOf(
                            team.read().config['provider'],
                          );
                        });
                      }
                    }),
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      TeamBar(
                        state: state,
                        member: member?['id'] as String?,
                        onTeam: () => setState(() => _choosingTeam = true),
                        onMember: (id) => _select(state, id),
                      ),
                      _heading(
                        context,
                        member == null
                            ? context.tr('teamLead')
                            : text(member['name']),
                        reset: member != null && state.overridden(role)
                            ? () => _run(() => team.onMember(role, null))
                            : null,
                        resettable: member != null,
                      ),
                      Expanded(
                        child: _models(
                          context,
                          selection: state.selection(member),
                          hint: member != null && model == null,
                          onChosen: member == null
                              ? null
                              : (choice) =>
                                    _run(() => team.onMember(role, choice)),
                        ),
                      ),
                    ],
                  ),
          ),
        );
      },
    );
  }

  Widget _heading(
    BuildContext context,
    String label, {
    required VoidCallback? reset,
    required bool resettable,
  }) {
    final colors = Theme.of(context).colorScheme;
    return SizedBox(
      height: 40,
      child: Row(
        children: [
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(fontSize: 13, color: colors.onSurfaceVariant),
            ),
          ),
          if (resettable)
            IconButton(
              key: const ValueKey('member-reset'),
              tooltip: context.tr('memberReset'),
              onPressed: reset,
              icon: const AppIcon('refresh', size: 16),
            ),
        ],
      ),
    );
  }

  /// [onChosen] receives a member's choice; without it the Lead's choice
  /// closes the sheet with a configuration change.
  Widget _models(
    BuildContext context, {
    required Map<String, dynamic> selection,
    bool hint = false,
    ValueChanged<Map<String, dynamic>>? onChosen,
  }) {
    final identity = (selection['provider'], selection['model']);
    if (_selectedModel != identity) {
      _selectedModel = identity;
      _familyKey = _familyOf(selection['provider']);
    }
    final colors = Theme.of(context).colorScheme;
    final visible = _providers.where((p) => _family(p) == _familyKey).toList();
    final list = ListView(
      padding: const EdgeInsets.only(bottom: 8),
      children: [
        for (final provider in visible) ...[
          InkWell(
            key: ValueKey('model-group-${provider['id']}'),
            borderRadius: BorderRadius.circular(10),
            onTap: () => setState(() {
              if (!_collapsed.remove(provider['id'])) {
                _collapsed.add(provider['id']);
              }
            }),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(8, 10, 8, 6),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      text(provider['name']),
                      style: TextStyle(
                        fontSize: 13,
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                  ),
                  AnimatedRotation(
                    turns: _collapsed.contains(provider['id']) ? -.25 : 0,
                    duration: const Duration(milliseconds: 150),
                    child: AppIcon(
                      'down',
                      size: 12,
                      color: colors.onSurfaceVariant,
                    ),
                  ),
                ],
              ),
            ),
          ),
          if (!_collapsed.contains(provider['id']))
            for (final model in objects(provider['models']))
              Builder(
                builder: (context) {
                  final selected =
                      provider['id'] == selection['provider'] &&
                      model['id'] == selection['model'];
                  return ChoiceRow(
                    label: _modelLabel(model),
                    selected: selected && !hint,
                    hint: selected && hint,
                    onTap: onChosen == null
                        ? () => Navigator.pop(context, {
                            'provider': provider['id'],
                            'credential': provider['credential'],
                            'model': model['id'],
                            'effort': model['default_effort'] ?? 'default',
                          })
                        : () => onChosen({
                            'provider': provider['id'],
                            'model': model['id'],
                          }),
                  );
                },
              ),
        ],
      ],
    );
    return _families.length < 2
        ? list
        : Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            spacing: 12,
            children: [
              _rail(context, colors),
              Expanded(child: list),
            ],
          );
  }

  Widget _rail(BuildContext context, ColorScheme colors) => Container(
    width: 52,
    padding: const EdgeInsets.all(4),
    decoration: BoxDecoration(
      color: colors.onSurface.withValues(alpha: .045),
      borderRadius: BorderRadius.circular(26),
    ),
    child: ListView(
      children: [
        for (final family in _families)
          Padding(
            padding: const EdgeInsets.only(bottom: 4),
            child: Builder(
              builder: (context) {
                final selected = family == _familyKey;
                final name = text(
                  _providers.firstWhere((p) => _family(p) == family)['name'],
                );
                return Tooltip(
                  message: name,
                  child: Material(
                    color: selected
                        ? colors.surfaceContainerHigh
                        : Colors.transparent,
                    shape: const CircleBorder(),
                    clipBehavior: Clip.antiAlias,
                    child: InkWell(
                      key: ValueKey('model-family-$family'),
                      onTap: () => setState(() => _familyKey = family),
                      child: SizedBox.square(
                        dimension: 44,
                        child: Center(
                          child: Text(
                            name.characters.firstOrNull?.toUpperCase() ?? '?',
                            style: TextStyle(
                              fontSize: 16,
                              fontWeight: FontWeight.w600,
                              color: selected
                                  ? colors.onSurface
                                  : colors.onSurfaceVariant,
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
                );
              },
            ),
          ),
      ],
    ),
  );
}
