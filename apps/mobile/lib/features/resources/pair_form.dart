import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:pinput/pinput.dart';
import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../ui/form.dart';

class PairForm extends StatefulWidget {
  const PairForm({super.key, required this.session});
  final AppSession session;
  @override
  State<PairForm> createState() => _PairFormState();
}

class _PairFormState extends State<PairForm> {
  final _form = GlobalKey<FormState>();
  final _pin = TextEditingController();
  bool _busy = false;
  String? _failure;
  @override
  void dispose() {
    _pin.dispose();
    super.dispose();
  }

  Future<void> _connect() async {
    if (_busy || !_form.currentState!.validate()) return;
    setState(() {
      _busy = true;
      _failure = null;
    });
    try {
      final host = await widget.session.pairPin(_pin.text);
      widget.session.selectHost(host.id);
      if (mounted) Navigator.pop(context);
    } catch (error) {
      debugPrint('Pairing failed: $error');
      if (mounted) {
        setState(
          () => _failure = error.toString().contains('NotFound:')
              ? 'pairExpired'
              : 'pairFailed',
        );
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.session,
    builder: (context, _) => Form(
      key: _form,
      child: FormBody(
        children: [
          LayoutBuilder(
            builder: (context, constraints) {
              final colors = Theme.of(context).colorScheme;
              final pinTheme = PinTheme(
                width: ((constraints.maxWidth - 30) / 6).clamp(28, 52),
                height: 54,
                textStyle: Theme.of(context).textTheme.titleLarge,
                decoration: BoxDecoration(
                  color: colors.surfaceContainer,
                  borderRadius: BorderRadius.circular(12),
                  border: Border.all(color: colors.outlineVariant),
                ),
              );
              return Semantics(
                label: context.tr('pairCode'),
                child: Pinput(
                  controller: _pin,
                  length: 6,
                  showErrorWhenFocused: true,
                  defaultPinTheme: pinTheme,
                  focusedPinTheme: pinTheme.copyDecorationWith(
                    border: Border.all(color: colors.primary),
                  ),
                  errorPinTheme: pinTheme.copyDecorationWith(
                    border: Border.all(color: colors.error),
                  ),
                  separatorBuilder: (_) => const SizedBox(width: 6),
                  enabled: !_busy,
                  autofocus: true,
                  keyboardType: TextInputType.number,
                  textInputAction: TextInputAction.done,
                  inputFormatters: [FilteringTextInputFormatter.digitsOnly],
                  validator: (value) => RegExp(r'^\d{6}$').hasMatch(value ?? '')
                      ? null
                      : context.tr('pairInvalid'),
                  onSubmitted: (_) {
                    if (widget.session.ready) _connect();
                  },
                ),
              );
            },
          ),
          if (_failure != null)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Text(
                context.tr(_failure!),
                style: TextStyle(color: Theme.of(context).colorScheme.error),
              ),
            ),
          FilledButton(
            onPressed: _busy || !widget.session.ready ? null : _connect,
            child: Text(context.tr(_busy ? 'pairing' : 'pairAction')),
          ),
        ],
      ),
    ),
  );
}
