import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/speech.dart';
import 'live.dart';
import '../../ui/loading.dart';

class SpeechSheet extends StatelessWidget {
  const SpeechSheet({super.key, required this.speech});
  final SpeechController speech;
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: speech,
    builder: (context, _) => Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SwitchListTile.adaptive(
          contentPadding: EdgeInsets.zero,
          title: Text(tr('speechInput')),
          value: speech.enabled,
          onChanged: speech.setEnabled,
        ),
        DropdownButtonFormField<String>(
          initialValue: speech.language,
          decoration: InputDecoration(labelText: tr('settingsSpeechLanguage')),
          items: [
            for (final item in [
              ('auto', 'settingsSpeechAuto'),
              ('zh', 'settingsSpeechChinese'),
              ('en', 'settingsSpeechEnglish'),
            ])
              DropdownMenuItem(value: item.$1, child: Text(tr(item.$2))),
          ],
          onChanged: (value) {
            if (value != null) speech.setLanguage(value);
          },
        ),
        const SizedBox(height: 20),
        if (speech.ready)
          Text(tr('settingsSpeechReady'))
        else if (speech.downloading) ...[
          LoadingOverlay(
            loading: true,
            progress: speech.progress / 100,
            child: const SizedBox(height: 96, width: double.infinity),
          ),
          TextButton(onPressed: speech.cancel, child: Text(tr('cancel'))),
        ] else
          FilledButton(
            onPressed: speech.download,
            child: Text(tr('settingsSpeechDownload')),
          ),
        settingsError(speech.error == null ? null : tr('settingsSpeechFailed')),
      ],
    ),
  );
}
