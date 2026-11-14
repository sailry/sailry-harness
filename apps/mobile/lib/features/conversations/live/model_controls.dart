import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import 'choice_row.dart';
import 'team.dart';
import 'team_controls.dart';

/// Edits a configuration draft; the owning form admits the revision to the Node.
/// With a team, the model and effort act on the selected avatar.
class ModelControls extends StatefulWidget {
  const ModelControls({
    super.key,
    required this.providers,
    required this.config,
    required this.onChanged,
    this.onCommitted,
    this.enabled = true,
    this.team,
    this.onTeam,
  });

  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> config;
  final ValueChanged<Map<String, dynamic>> onChanged;
  final ValueChanged<Map<String, dynamic>>? onCommitted;
  final bool enabled;

  /// Team choices for a live session or a new conversation draft.
  final TeamChoices? team;

  /// Opens the team list in place of the controls.
  final VoidCallback? onTeam;

  @override
  State<ModelControls> createState() => _ModelControlsState();
}

class _ModelControlsState extends State<ModelControls> {
  /// The selected member's role ID; null selects the Lead.
  String? _member;

  /// A member's effort while the slider moves, before admission.
  String? _effort;

  void _commit(Map<String, dynamic> value) {
    widget.onChanged(value);
    widget.onCommitted?.call(value);
  }

  Future<void> _setMember(String role, Map<String, dynamic>? model) async {
    await widget.team!.onMember(role, model);
    if (mounted) setState(() => _effort = null);
  }

  @override
  Widget build(BuildContext context) {
    final team = widget.team;
    if (team == null) return _controls(context, null, null);
    return ListenableBuilder(
      listenable: team.listenable,
      builder: (context, _) {
        final state = team.read();
        final member = state.members
            .where((m) => m['id'] == _member)
            .firstOrNull;
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            TeamBar(
              state: state,
              member: member?['id'] as String?,
              onTeam: widget.onTeam ?? () {},
              onMember: (id) => setState(() {
                _member = id;
                _effort = null;
              }),
            ),
            const SizedBox(height: 8),
            _controls(context, state, member),
          ],
        );
      },
    );
  }

  Widget _controls(
    BuildContext context,
    SessionTeam? state,
    Map<String, dynamic>? member,
  ) {
    final enabled = widget.enabled;
    final config = widget.config;
    final role = text(member?['id']);
    final selection = member == null ? config : state!.selection(member);
    final provider = widget.providers
        .where((p) => p['id'] == selection['provider'])
        .firstOrNull;
    final model = objects(
      provider?['models'],
    ).where((m) => m['id'] == selection['model']).firstOrNull;
    final efforts = <String>{
      ...(model?['efforts'] as List? ?? []).whereType<String>(),
    }.toList();
    final features = member == null
        ? objects(object(model?['capabilities'])['features'])
        : state!.featureOptions(selection);
    final chosen = member == null
        ? object(selection['features'])
        : state!.features(member);
    final initial = model?['default_effort'] ?? 'default';
    final current = member == null
        ? config['effort'] ?? 'default'
        : _effort ?? selection['effort'] ?? initial;
    final selected = efforts.indexOf(current is String ? current : '');
    final budget = object(current)['budget'];
    final label = budget == null
        ? text(current)
        : '${context.tr('conversationBudget')}: $budget';
    final colors = Theme.of(context).colorScheme;
    // A member resets its override; the Lead resets its effort.
    final reset = member == null
        ? current != initial
              ? () => _commit({...config, 'effort': initial})
              : null
        : state!.overridden(role)
        ? () => _setMember(role, null)
        : null;
    final name = member == null ? context.tr('teamLead') : text(member['name']);
    final resetButton = IconButton(
      key: const ValueKey('model-reset'),
      style: IconButton.styleFrom(
        minimumSize: const Size.square(40),
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
      tooltip: context.tr(member == null ? 'resetReasoning' : 'memberReset'),
      onPressed: enabled ? reset : null,
      icon: const AppIcon('refresh', size: 20),
    );
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            if (state != null)
              Expanded(
                child: Tooltip(
                  message: name,
                  child: Text(
                    name,
                    key: const ValueKey('model-role-name'),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                      color: colors.onSurfaceVariant,
                    ),
                  ),
                ),
              )
            else
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
            if (state != null)
              Expanded(
                child: Align(
                  alignment: Alignment.centerRight,
                  child: resetButton,
                ),
              )
            else
              resetButton,
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
                  final choice = await showAppSheet<Map<String, dynamic>>(
                    context,
                    context.tr('modelPicker'),
                    scroll: false,
                    child: ModelChoices(
                      providers: widget.providers,
                      selection: selection,
                      // A member without a model follows the Lead.
                      hint: member != null && state!.model(member) == null,
                    ),
                  );
                  if (choice == null || !mounted) return;
                  if (member == null) {
                    // The form owns the draft, including edits made while open.
                    _commit({...widget.config, ...choice});
                  } else {
                    await _setMember(role, state!.choose(member, choice));
                  }
                },
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Flexible(
                child: Text(
                  chipLabel(null, selection, context.tr),
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              const SizedBox(width: 8),
              const AppIcon('chevron', size: 16),
            ],
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
                  ? (value) {
                      final effort = efforts[value.round()];
                      if (member == null) {
                        widget.onChanged({...config, 'effort': effort});
                      } else {
                        setState(() => _effort = effort);
                      }
                    }
                  : null,
              onChangeEnd: enabled && efforts.length > 1
                  ? (value) {
                      final effort = efforts[value.round()];
                      if (member == null) {
                        widget.onCommitted?.call({...config, 'effort': effort});
                      } else if (effort != (selection['effort'] ?? initial)) {
                        _setMember(role, state!.effort(member, effort));
                      } else {
                        setState(() => _effort = null);
                      }
                    }
                  : null,
            ),
          ),
        ],
        for (final feature in features)
          if (objects(feature['options']).isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 8),
              child: _FeatureChoice(
                feature: feature,
                selected: text(chosen[feature['id']]),
                enabled: enabled,
                onSelected: (value) {
                  if (member == null) {
                    final next = {...chosen}..remove(feature['id']);
                    if (value.isNotEmpty) next[text(feature['id'])] = value;
                    _commit({...config, 'features': next});
                  } else {
                    _setMember(
                      role,
                      state!.feature(member, text(feature['id']), value),
                    );
                  }
                },
              ),
            ),
      ],
    );
  }
}

String _modelLabel(Map<String, dynamic> model) =>
    text(model['display_name'], text(model['id']));

/// Providers sharing a preset form one family, like the Desktop rail.
String _family(Map<String, dynamic> provider) {
  final preset = text(object(provider['preset'])['id']);
  return preset.isEmpty ? text(provider['name']) : preset;
}

/// Desktop-style picker: a family rail beside collapsible provider groups.
/// Closes with the chosen provider, credential, model and default effort.
class ModelChoices extends StatefulWidget {
  const ModelChoices({
    super.key,
    required this.providers,
    required this.selection,
    this.hint = false,
  });
  final List<Map<String, dynamic>> providers;
  final Map<String, dynamic> selection;

  /// Marks the selection as inherited rather than chosen.
  final bool hint;

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
  late String? _familyKey = _familyOf(widget.selection['provider']);
  final _collapsed = <Object?>{};

  String? _familyOf(Object? provider) {
    final current = _providers.where((p) => p['id'] == provider).firstOrNull;
    return current == null ? _families.firstOrNull : _family(current);
  }

  @override
  Widget build(BuildContext context) {
    if (_providers.isEmpty) {
      return EmptyState(message: context.tr('conversationNoModel'));
    }
    final selection = widget.selection;
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
                    selected: selected && !widget.hint,
                    hint: selected && widget.hint,
                    onTap: () => Navigator.pop(context, {
                      'provider': provider['id'],
                      'credential': provider['credential'],
                      'model': model['id'],
                      'effort': model['default_effort'] ?? 'default',
                    }),
                  );
                },
              ),
        ],
      ],
    );
    return SizedBox(
      height: MediaQuery.sizeOf(context).height * .6,
      child: _families.length < 2
          ? list
          : Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              spacing: 12,
              children: [
                _rail(context, colors),
                Expanded(child: list),
              ],
            ),
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

/// One model-declared choice, such as Speed with Fast and Ultrafast, beside
/// an off segment. Desktop shows the same declarations as composer toggles.
class _FeatureChoice extends StatelessWidget {
  const _FeatureChoice({
    required this.feature,
    required this.selected,
    required this.enabled,
    required this.onSelected,
  });

  final Map<String, dynamic> feature;

  /// The selected option ID; empty when the feature is off.
  final String selected;
  final bool enabled;
  final ValueChanged<String> onSelected;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final speed = feature['id'] == 'speed';
    final options = objects(feature['options']);
    String label(Map<String, dynamic> option) => speed && option['id'] == 'fast'
        ? context.tr('modelFeatureFast')
        : text(option['name']).isEmpty
        ? text(option['id'])
        : text(option['name']);
    final name = speed
        ? context.tr('modelFeatureSpeed')
        : text(feature['name']).isEmpty
        ? text(feature['id'])
        : text(feature['name']);
    final current = options.any((option) => option['id'] == selected)
        ? selected
        : '';
    return Row(
      children: [
        SizedBox(
          width: 88,
          child: Text(
            name,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(fontSize: 15, color: colors.onSurface),
          ),
        ),
        Expanded(
          child: SegmentedButton<String>(
            key: ValueKey('model-feature-${feature['id']}'),
            expandedInsets: EdgeInsets.zero,
            showSelectedIcon: false,
            segments: [
              ButtonSegment(
                value: '',
                label: Text(context.tr('modelFeatureOff')),
              ),
              for (final option in options)
                ButtonSegment(
                  value: text(option['id']),
                  label: Text(label(option), overflow: TextOverflow.ellipsis),
                ),
            ],
            selected: {current},
            onSelectionChanged: enabled
                ? (value) => onSelected(value.single)
                : null,
            style: SegmentedButton.styleFrom(
              side: BorderSide.none,
              backgroundColor: colors.surfaceContainer,
              selectedBackgroundColor: colors.surfaceContainerHigh,
              selectedForegroundColor: colors.onSurface,
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 10),
            ),
          ),
        ),
      ],
    );
  }
}
