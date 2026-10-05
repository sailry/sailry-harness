import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/speech.dart';
import 'live.dart';
import '../../ui/loading.dart';
import '../../ui/form.dart';

class SpeechSheet extends StatelessWidget {
  const SpeechSheet({super.key, required this.speech});
  final SpeechController speech;
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: speech,
    builder: (context, _) => FormBody(
      children: [
        SwitchListTile.adaptive(
          contentPadding: EdgeInsets.zero,
          title: Text(tr('speechInput')),
          value: speech.enabled,
          onChanged: speech.setEnabled,
        ),
        SelectField<String>(
          value: speech.language,
          label: tr('settingsSpeechLanguage'),
          options: [
            for (final item in [
              ('auto', 'settingsSpeechAuto'),
              ('zh', 'settingsSpeechChinese'),
              ('en', 'settingsSpeechEnglish'),
            ])
              (item.$1, tr(item.$2)),
          ],
          onChanged: speech.setLanguage,
        ),
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
        if (speech.error != null) settingsError(tr('settingsSpeechFailed')),
      ],
    ),
  );
}
