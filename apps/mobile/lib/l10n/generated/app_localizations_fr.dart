// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for French (`fr`).
class AppLocalizationsFr extends AppLocalizations {
  AppLocalizationsFr([String locale = 'fr']) : super(locale);

  @override
  String get updatesVersion => 'Version';

  @override
  String get updatesCheck => 'Vérifier les mises à jour';

  @override
  String get updatesChecking => 'Vérification en cours';

  @override
  String updatesAvailable(String version) {
    return 'La version $version est disponible';
  }

  @override
  String get updatesDownload => 'Télécharger la mise à jour';

  @override
  String get updatesCurrent => 'Vous êtes à jour';

  @override
  String get updatesUnpublished => 'Aucune version mobile pour le moment';

  @override
  String get updatesCheckFailed => 'Impossible de vérifier les mises à jour';

  @override
  String get updatesOpenFailed => 'Impossible d\'ouvrir le téléchargement';

  @override
  String get retryTask => 'Réessayer';

  @override
  String get welcomeTitle => 'Sur quoi aimeriez-vous travailler aujourd’hui?';

  @override
  String get welcomeExplore => 'Découvrir un projet';

  @override
  String get welcomeExploreDetail =>
      'Comprendre sa structure et ses points d’entrée';

  @override
  String get welcomeExplorePrompt =>
      'Aidez-moi à comprendre ce projet, y compris ses modules clés et ses points d\'entrée.';

  @override
  String get welcomeBuild => 'Construire une idée';

  @override
  String get welcomeBuildDetail => 'Donnez vie à votre idée';

  @override
  String get welcomeBuildPrompt =>
      'Je veux ajouter une fonctionnalité à ce projet, confirmez d\'abord les exigences avec moi et esquissez un plan de mise en œuvre.';

  @override
  String get welcomeReview => 'Examen des changements';

  @override
  String get welcomeReviewDetail =>
      'Vérifier les changements et les problèmes potentiels';

  @override
  String get welcomeReviewPrompt =>
      'Examiner les changements actuels dans ce projet, en se concentrant sur les problèmes potentiels et les tests manquants.';

  @override
  String get welcomePlan => 'Faire un plan';

  @override
  String get welcomePlanDetail => 'Clarifier les objectifs et les étapes';

  @override
  String get welcomePlanPrompt =>
      'Aidez-moi à créer un plan étape par étape pour les travaux de développement à venir.';

  @override
  String get conversationEmpty => 'Décrivez votre tâche';

  @override
  String get conversationLoading => 'Chargement du chat';

  @override
  String get conversationReconnecting => 'Reconnexion en cours';

  @override
  String get conversationErrorDetails => 'Voir la raison';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return 'Réessai de la demande de modèle $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return 'Modèle de demande réessayé $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count appels d\'outils';
  }

  @override
  String get conversationGoal => 'Objectif';

  @override
  String get conversationGoalBlocked => 'Bloqué';

  @override
  String conversationGoalBudget(String count) {
    return 'Budget $count jetons';
  }

  @override
  String get conversationStepSkipped => 'Ignoré';

  @override
  String get conversationChild => 'Sous-tâche';

  @override
  String get conversationChildReadonly => 'Sous-tâche de chat';

  @override
  String get conversationOffline => 'Connexion perdue';

  @override
  String get conversationUnavailable => 'Chat non disponible';

  @override
  String get conversationFailed => 'Opération a échoué; réessayez';

  @override
  String get conversationUnknown =>
      'Résultat non confirmé; vérifiez le chat avant d\'agir à nouveau';

  @override
  String get conversationCheckResult => 'Vérifier le résultat';

  @override
  String get conversationConflict =>
      'Configuration modifiée; rouvrir avant d\'agir à nouveau';

  @override
  String get conversationOlder => 'Charger les anciens messages';

  @override
  String get conversationNew => 'Un nouveau chat';

  @override
  String get conversationNoHost => 'Connecter un hôte en premier';

  @override
  String get conversationNoProject =>
      'Ajouter un projet sur l\'hôte en premier';

  @override
  String get conversationNoModel =>
      'Configurer un modèle sur l\'hôte en premier';

  @override
  String get conversationNoTasks => 'Pas encore de chats';

  @override
  String get conversationNoMessages => 'Pas de messages';

  @override
  String get conversationPreviewUnavailable => 'Message indisponible';

  @override
  String get conversationInterrupted => 'Interrompu';

  @override
  String get conversationFailedStatus => 'Échec';

  @override
  String get conversationStopping => 'Arrêt en cours';

  @override
  String get conversationQueued => 'En file d\'attente';

  @override
  String get conversationProcessing => 'Traitement';

  @override
  String get conversationUnsynced => 'État non synchronisé';

  @override
  String get conversationGenerating => 'Réponse';

  @override
  String get conversationWaiting => 'En attente de confirmation';

  @override
  String get conversationCompacting => 'Compacter le contexte';

  @override
  String get conversationForkConfirm =>
      'Fork un chat à partir de cet enregistrement?';

  @override
  String get conversationCompacted => 'Contexte compacté';

  @override
  String get conversationToolWaiting => 'En attente';

  @override
  String get conversationToolRunning => 'En cours';

  @override
  String get conversationToolReturned => 'Retourné';

  @override
  String get conversationToolCancelled => 'Annulé';

  @override
  String get conversationToolNotExecuted => 'Non exécutées';

  @override
  String get conversationToolInterrupted => 'Interrompu';

  @override
  String get conversationUnsupportedInput => 'Gérer cette entrée sur le bureau';

  @override
  String get conversationStartCoding => 'Démarrer l\'exécution';

  @override
  String get conversationPlanFeedback => 'Suggérer des changements';

  @override
  String get conversationOther => 'Autre';

  @override
  String get conversationSubmit => 'Envoyer';

  @override
  String get conversationSource => 'Source';

  @override
  String get conversationMode => 'Mode de travail';

  @override
  String get conversationCode => 'Exécuter';

  @override
  String get conversationPlan => 'Plan';

  @override
  String get conversationPermission => 'Permissions';

  @override
  String get conversationAsk => 'Demandez à chaque fois';

  @override
  String get conversationProject => 'Accès au projet';

  @override
  String get conversationFull => 'Un accès complet';

  @override
  String get conversationReasoning => 'Effort de raisonnement';

  @override
  String get conversationDefault => 'Par défaut';

  @override
  String get conversationNone => 'Désactivé';

  @override
  String get conversationMinimal => 'Minimal';

  @override
  String get conversationLow => 'Faible';

  @override
  String get conversationMedium => 'Moyen';

  @override
  String get conversationHigh => 'Élevé';

  @override
  String get conversationXHigh => 'Supérieur';

  @override
  String get conversationMax => 'Maximum';

  @override
  String get conversationBudget => 'Raisonnement budgétaire';

  @override
  String get conversationAttachment => 'Pièce jointe';

  @override
  String get conversationAttachmentTooLarge =>
      'Pièce jointe illisible ou plus grande que 64 Mo';

  @override
  String get conversationDownload => 'Voir pièce jointe';

  @override
  String get conversationImageFailed => 'Impossible d\'afficher l\'image';

  @override
  String get conversationDownloadFailed =>
      'Impossible de charger la pièce jointe';

  @override
  String get conversationReadonly => 'Ce chat est archivé';

  @override
  String get conversationMicrophoneDenied =>
      'Impossible d\'accéder au microphone';

  @override
  String get conversationRecordingFailed => 'La reconnaissance vocale a échoué';

  @override
  String get conversationSpeechDisabled => 'L\'entrée vocale est désactivée';

  @override
  String get conversationSpeechMissing =>
      'Téléchargez d\'abord le modèle de discours dans Paramètres';

  @override
  String get conversationRecording => 'Enregistrement';

  @override
  String get conversationTranscribing => 'Transcription';

  @override
  String get conversationRecordReady => 'Prêt à enregistrer';

  @override
  String get conversationStartRecording => 'Commencer l\'enregistrement';

  @override
  String get conversationFinishRecording => 'Terminer l\'enregistrement';

  @override
  String get conversationSources => 'Sources';

  @override
  String get conversationSearchSuggestions => 'Suggestions de recherche';

  @override
  String get conversationStats => 'Utilisation du chat';

  @override
  String get conversationStatsEmpty => 'Aucune utilisation pour le moment';

  @override
  String get conversationStatsOverview => 'Aperçu général';

  @override
  String get conversationStatsTokenGroup => 'Utilisation des jetons';

  @override
  String get conversationStatsCostGroup => 'Coût';

  @override
  String get conversationStatsGenerationGroup => 'Génération';

  @override
  String get conversationStatsTokens => 'Tokens';

  @override
  String get conversationStatsInput => 'Entrée';

  @override
  String get conversationStatsOutput => 'Sortie';

  @override
  String get conversationStatsCached => 'Entrée mise en cache';

  @override
  String get conversationStatsReasoning => 'Raisonnement de sortie';

  @override
  String get conversationStatsCacheRate => 'Hits de cache';

  @override
  String get conversationStatsCost => 'Coût estimatif';

  @override
  String get conversationStatsCostCoverage => 'Couverture des coûts';

  @override
  String get conversationStatsSpeed => 'Vitesse de génération';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => 'Couverture temporelle';

  @override
  String get conversationStatsTurns => 'Tours';

  @override
  String get conversationStatsResponses => 'Modèle de réponses';

  @override
  String get conversationStatsContext => 'Le contexte actuel';

  @override
  String get conversationStatsInputCost => 'Coût des intrants';

  @override
  String get conversationStatsOutputCost => 'Coût de production';

  @override
  String get conversationStatsCacheReadCost => 'Coût de lecture du cache';

  @override
  String get conversationStatsCacheWriteCost =>
      'Coût d &apos; écriture du cache';

  @override
  String get messageHistoryUpdated =>
      'Chat mis à jour; enregistrements originaux conservés';

  @override
  String get turnUndoUnsaved =>
      'Les fichiers ont des modifications non enregistrées; enregistrez-les ou abandonnez-les en premier';

  @override
  String get codePlain => 'Texte brut';

  @override
  String get toolArguments => 'Arguments';

  @override
  String get toolResult => 'Résultat';

  @override
  String get toolRaw => 'Résultat brut';

  @override
  String get turnChanges => 'Tourner les changements';

  @override
  String turnChangesCount(String count) {
    return '$count fichiers';
  }

  @override
  String get turnUndo => 'Annuler les modifications';

  @override
  String get turnUndoAll => 'Annuler tout';

  @override
  String get turnUndoConfirm =>
      'Annuler ces modifications de fichier à partir de ce tour? Les conflits avec des modifications ultérieures arrêteront l\'opération';

  @override
  String get turnUndoDone => 'Annulé';

  @override
  String get turnUndoPartial =>
      'Certaines modifications annulées; vérifier les fichiers restants';

  @override
  String get messageActions => 'Actions de message';

  @override
  String get messageEdit => 'Modifier et régénérer';

  @override
  String get messageEditConfirm =>
      'Remplacer ce message et le chat suivant? Les fichiers ne seront pas restaurés';

  @override
  String get messageRewind => 'Rebobinez ici';

  @override
  String get messageRewindConfirm =>
      'Les enregistrements de discussions ultérieurs seront sauvegardés; les fichiers ne seront pas restaurés';

  @override
  String get messageBackup => 'Voir la sauvegarde du chat';

  @override
  String get messageRegenerate => 'Régénérer';

  @override
  String get messageSearch => 'Rechercher dans le chat';

  @override
  String get messageSearchHint => 'Rechercher des messages';

  @override
  String get messageSearchMissing =>
      'Ce message n\'est plus dans le chat actuel';

  @override
  String get messageSearchStale => 'Le chat a changé; recherchez à nouveau';

  @override
  String get messageNoResults => 'Aucun message correspondant';

  @override
  String get messageCheck => 'Vérifier le résultat de l\'opération';

  @override
  String get messageReference => 'Référence';

  @override
  String get messageReferenceContext =>
      'Cette référence appartient au contexte où le message a été envoyé';

  @override
  String get toolFailed => 'Échec';

  @override
  String toolExitCode(String code) {
    return 'Code de sortie $code';
  }

  @override
  String toolSignal(String signal) {
    return 'Terminé par le signal $signal';
  }

  @override
  String get toolTimedOut => 'Commande dépassée';

  @override
  String get toolCancelled => 'Commande annulée';

  @override
  String get toolOutcomeUnknown => 'Résultat du commandement inconnu';

  @override
  String get toolQuestionAnswered => 'Réponse';

  @override
  String get toolQuestionDeclined => 'Demande rejetée';

  @override
  String get toolQuestionCancelled => 'Annulé';

  @override
  String get fileLinkUnavailable => 'Impossible d\'ouvrir ce lien';

  @override
  String get imagePreview => 'Aperçu de l\'image';

  @override
  String get fileOpenExternal => 'Ouvrir avec une autre application';

  @override
  String get fileOpenFailed => 'Impossible d\'ouvrir le fichier';

  @override
  String get fileNoApplication =>
      'Aucune application ne peut ouvrir ce fichier';

  @override
  String get fileSaveBeforeShare =>
      'Sauvegarder les modifications avant de les partager?';

  @override
  String fileTrashConfirm(String name) {
    return 'Déplacer « $name » dans la corbeille de l’hôte? Les modifications non enregistrées seront également supprimées';
  }

  @override
  String get fileTrashUncertain =>
      'Résultat de suppression non confirmé; réessayer la requête';

  @override
  String get fileSaveFailed => 'Sauvegarde du fichier a échoué';

  @override
  String get terminalHideKeyboard => 'Masquer le clavier';

  @override
  String get terminalEscape => 'Esc';

  @override
  String get terminalTab => 'Tab';

  @override
  String get terminalCtrl => 'Ctrl';

  @override
  String get terminalAlt => 'Alt';

  @override
  String get terminalShift => 'Shift';

  @override
  String get terminalCmd => 'Cmd';

  @override
  String get terminalArrowLeft => 'Gauche';

  @override
  String get terminalArrowUp => 'Haut';

  @override
  String get terminalArrowDown => 'Bas';

  @override
  String get terminalArrowRight => 'Droite';

  @override
  String get resourceNoWorkspace =>
      'Choisir une arborescence de travail sur un hôte connecté';

  @override
  String get resourceDisconnected => 'Hébergeur non connecté';

  @override
  String get resourceRoot => 'Racine';

  @override
  String get resourceMore => 'Charger plus d\'articles';

  @override
  String get resourcePartial => 'Contenu partiel montré';

  @override
  String get resourceEmpty => 'Pas de contenu';

  @override
  String get resourceSaveError =>
      'Échec de l\'enregistrement; brouillon conservé';

  @override
  String get resourceReloadConfirm =>
      'Jeter le brouillon et charger le dernier contenu?';

  @override
  String get resourceWorktreeCreate => 'Nouvel arbre de travail';

  @override
  String get resourceSessionServices => 'Services de chat';

  @override
  String get resourceServicesUnavailable => 'Liste de services indisponible';

  @override
  String get resourceNoServices => 'Aucune adresse de service trouvée';

  @override
  String get resourceServiceOpen => 'Service ouvert';

  @override
  String get resourceRemotePort => 'Port distant';

  @override
  String get resourceOpenPort => 'Port avant';

  @override
  String get resourceOpenBrowser => 'Prévisualiser la page web';

  @override
  String get resourcePreviewFailed => 'Impossible de charger la page';

  @override
  String get resourcePreviewLink => 'Impossible d\'ouvrir ce lien en aperçu';

  @override
  String get resourceForwardStopped => 'Transfert arrêté';

  @override
  String get resourceTerminalControl => 'Prenez le contrôle';

  @override
  String get resourceTerminalControlHint => 'Contrôlé par un autre appareil';

  @override
  String get resourceTerminalClaiming => 'Prendre le contrôle';

  @override
  String get resourceTerminalReadOnly => 'Terminal en lecture seule';

  @override
  String get resourceTerminalEnded => 'Terminaux fermés';

  @override
  String get resourceTerminalConnecting => 'Terminal de raccordement';

  @override
  String get resourceTerminalInput => 'Entrée de terminal';

  @override
  String get resourceTerminalPaste => 'Coller';

  @override
  String get resourceGitNotRepository =>
      'Ce répertoire n\'est pas un dépôt Git';

  @override
  String get resourceInvalidPort => 'Entrez un port compris entre 1 et 65535';

  @override
  String get tool_navigate => 'Ouvrir la page';

  @override
  String get tool_back => 'Retourner en arrière';

  @override
  String get tool_forward => 'Aller de l\'avant';

  @override
  String get tool_refresh => 'Actualiser la page';

  @override
  String get tool_right_click => 'Élément de clic';

  @override
  String get tool_clear => 'Entrez le texte';

  @override
  String get tool_select => 'Sélectionner une option';

  @override
  String get tool_hover => 'Élément Hover';

  @override
  String get tool_scroll => 'Défiler la page';

  @override
  String get tool_press_key => 'Appuyez sur la touche';

  @override
  String get tool_new_tab => 'Nouvel onglet';

  @override
  String get tool_list_windows => 'Onglets du navigateur';

  @override
  String get tool_switch_window => 'Changer l\'onglet';

  @override
  String get tool_close_window => 'Fermer la fenêtre';

  @override
  String get tool_close_session => 'Fermer le navigateur';

  @override
  String get tool_screenshot => 'Capture de page';

  @override
  String get tool_print_to_pdf => 'Exporter au format PDF';

  @override
  String get tool_file_upload => 'Télécharger le fichier';

  @override
  String get tool_downloads => 'Voir les téléchargements';

  @override
  String get tool_save_download => 'Enregistrer le fichier téléchargé';

  @override
  String get tool_evaluate_js => 'Exécuter un script de page';

  @override
  String get tool_get_cookies => 'Lire les cookies';

  @override
  String get tool_delete_all_cookies => 'Modifier les cookies';

  @override
  String get tool_drag_and_drop => 'Faites glisser l\'élément';

  @override
  String get tool_focus => 'Élément focal';

  @override
  String get tool_handle_alert => 'Alerte de page de gestion';

  @override
  String get tool_database_catalog => 'Parcourir la base de données';

  @override
  String get tool_database_query => 'Base de données de requête';

  @override
  String get tool_database_execute =>
      'Exécuter l\'opération de base de données';

  @override
  String get tool_search_memory => 'Mémoire de recherche';

  @override
  String get tool_review_memories => 'Réviser les souvenirs';

  @override
  String get tool_consolidate_memories => 'Fusionner les souvenirs';

  @override
  String get tool_save_memory => 'Économisez de la mémoire';

  @override
  String get tool_forget_memory => 'Supprimer la mémoire';

  @override
  String get tool_update_plan => 'Mettre à jour le plan';

  @override
  String get tool_create_goal => 'Créer un objectif';

  @override
  String get tool_get_goal => 'Afficher l’objectif';

  @override
  String get tool_update_goal => 'Objectif de mise à jour';

  @override
  String get tool_spawn_agent => 'Sous-agent';

  @override
  String get tool_browser_tabs => 'Onglets du navigateur';

  @override
  String get tool_browser_read => 'Lire la page';

  @override
  String get tool_browser_navigate => 'Ouvrir la page';

  @override
  String get tool_browser_click => 'Élément de clic';

  @override
  String get tool_browser_input => 'Entrez le texte';

  @override
  String get tool_browser_scroll => 'Défiler la page';

  @override
  String get tool_browser_back => 'Retourner en arrière';

  @override
  String get tool_browser_forward => 'Aller de l\'avant';

  @override
  String get tool_browser_refresh => 'Actualiser la page';

  @override
  String get tool_browser_open => 'Nouvel onglet';

  @override
  String get tool_browser_close => 'Fermer l\'onglet';

  @override
  String get tool_browser_focus => 'Changer l\'onglet';

  @override
  String get tool_browser_select => 'Sélectionner une option';

  @override
  String get tool_browser_hover => 'Élément Hover';

  @override
  String get tool_browser_key => 'Appuyez sur la touche';

  @override
  String get tool_browser_frame => 'Cadre de commutateur';

  @override
  String get tool_browser_wait => 'Attendre la page';

  @override
  String get tool_browser_screenshot => 'Capture de page';

  @override
  String get tool_ssh_run => 'Exécuter la commande SSH';

  @override
  String get tool_ssh_transfer => 'Transfert SSH fichier';

  @override
  String get tool_list_worktrees => 'Liste des arbres de travail';

  @override
  String get tool_create_worktree => 'Nouvel arbre de travail';

  @override
  String get tool_register_worktree => 'Ajouter un arbre de travail';

  @override
  String get tool_remove_worktree => 'Supprimer l\'arbre de travail';

  @override
  String get tool_google_search => 'Recherche sur le web';

  @override
  String get tool_web_fetch => 'Récupérer la page';

  @override
  String get tool_fetch_url => 'Récupérer la page';

  @override
  String get tool_read_file => 'Lire le fichier';

  @override
  String get tool_write_file => 'Écrire le fichier';

  @override
  String get tool_list_directory => 'Parcourir le répertoire';

  @override
  String get tool_search_files => 'Recherche de fichiers';

  @override
  String get tool_run_command => 'Exécuter la commande';

  @override
  String get tool_read_command => 'Afficher l\'arrière-plan commande';

  @override
  String get tool_stop_command => 'Commande d\'arrêt';

  @override
  String get tool_load_skill => 'Capacité de chargement';

  @override
  String get tool_read_skill_resource => 'Lire la ressource de compétence';

  @override
  String get tool_computer_desktop => 'Voir le bureau';

  @override
  String get tool_computer_observe => 'Observer l\'écran';

  @override
  String get tool_computer_input => 'Ordinateur de contrôle';

  @override
  String get tool_computer_focus => 'L\'application Switch';

  @override
  String get tool_computer_open => 'Ouvrir l\'appli';

  @override
  String get tool_git_status => 'Git status';

  @override
  String get tool_git_diff => 'Afficher les différences';

  @override
  String get tool_git_log => 'Git log';

  @override
  String get tool_inspect_image => 'Inspecter l\'image';

  @override
  String get tool_generate_image => 'Générer une image';

  @override
  String get tool_generate_video => 'Générer une vidéo';

  @override
  String get terminalUnavailable => 'Le terminal n\'est pas connecté';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => 'Actualiser';

  @override
  String get loading => 'Chargement';

  @override
  String get home => 'Chats';

  @override
  String get idle => 'Inactif';

  @override
  String get allProjects => 'Tous les projets';

  @override
  String get allWorktrees => 'Tous les arbres de travail';

  @override
  String get filterProjects => 'Filtrer les projets';

  @override
  String get closeSearch => 'Fermer la recherche';

  @override
  String onlineHostCount(String count) {
    return '$count en ligne';
  }

  @override
  String get taskActions => 'Actions de tâche';

  @override
  String get archiveShort => 'Archives';

  @override
  String get archiveTab => 'Archivé';

  @override
  String get archivedTasks => 'Archivé';

  @override
  String get delete => 'Supprimer';

  @override
  String get deleteTask => 'Supprimer le chat';

  @override
  String get deleteWarning =>
      'Ce chat ne peut pas être repris après la suppression';

  @override
  String get busyDelete => 'Arrêter la tâche avant de supprimer ce chat';

  @override
  String get stopBeforeDelete => 'Arrêter la tâche';

  @override
  String get deleted => 'Chat supprimé de l\'aperçu';

  @override
  String get restored => 'Restauré à la maison';

  @override
  String get restore => 'Restaurer';

  @override
  String get archiveEmpty => 'Aucun chat archivé';

  @override
  String get archiveKeepsRunning =>
      'L\'archivage n\'arrête pas l\'exécution des tâches';

  @override
  String get title => 'Sailry · Aperçu mobile';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => 'Espace de travail mobile';

  @override
  String get edition => 'EXPLORATION MOBILE / 01';

  @override
  String get intro => 'Tâches, chats et espaces de travail distants';

  @override
  String get preview => 'Aperçu';

  @override
  String get sample => '· Les modifications restent sur cette page';

  @override
  String get mixed => 'Lumière et obscurité';

  @override
  String get dark => 'Sombre';

  @override
  String get light => 'Clair';

  @override
  String get gallery => 'Aperçu général';

  @override
  String get focus => 'Un seul écran';

  @override
  String get reset => 'Réinitialiser l\'aperçu';

  @override
  String get page => 'Choisir une page';

  @override
  String get experience => 'Ouvrir la page';

  @override
  String get backGallery => 'Retour à l\'aperçu';

  @override
  String get design => 'Caractéristiques et design';

  @override
  String get footer => 'VAILLY / MOBILE';

  @override
  String get footerNote => 'Aperçu HTML local · Aucune connexion de service';

  @override
  String get tasks => 'Tâches';

  @override
  String get chat => 'Chat';

  @override
  String get hosts => 'Hôtes';

  @override
  String get resources => 'Ressources';

  @override
  String get settings => 'Paramètres';

  @override
  String get usage => 'Utilisation';

  @override
  String get changes => 'Modifications';

  @override
  String get terminal => 'Terminal';

  @override
  String get newTerminal => 'Nouveau terminal';

  @override
  String get files => 'Fichiers';

  @override
  String get project => 'Projet';

  @override
  String get worktree => 'Arbre de travail';

  @override
  String get subtitleTasks =>
      'Tâches sur plusieurs hôtes · Approbations et réponses en premier';

  @override
  String get subtitleChat =>
      'Chat continu · Développez l\'activité de l\'outil au besoin';

  @override
  String get subtitleHosts => 'Connexions, ressources hôtes et processus';

  @override
  String get subtitleResources => 'Hébergeur → Projet → Arbre de travail';

  @override
  String get subtitleChanges =>
      'Différences de fichiers, mise en attente et validations';

  @override
  String get subtitleTerminal =>
      'Terminal à distance · Contrôle d\'entrée explicite';

  @override
  String get subtitleUsage =>
      'Sailry conversations · Agrégées sur tous les hôtes';

  @override
  String get subtitleSettings =>
      'Préférences locales et paramètres d\'exécution Node';

  @override
  String get allHosts => 'Tous les hôtes';

  @override
  String get connectedHosts => '2 en ligne';

  @override
  String get all => 'Tout';

  @override
  String get running => 'En cours';

  @override
  String get waiting => 'En attente';

  @override
  String get completed => 'Achevée';

  @override
  String get taskProgress => 'Tâche actuelle';

  @override
  String get taskWait => 'En attente de votre décision';

  @override
  String get taskRecent => 'Récemment terminé';

  @override
  String get search => 'Rechercher';

  @override
  String get searchTasks => 'Rechercher des tâches et des projets';

  @override
  String get filterTasks => 'Filtrer les tâches';

  @override
  String get noResults => 'Aucune tâche correspondante';

  @override
  String get notification => 'Notifications';

  @override
  String get newTask => 'Nouvelle tâche';

  @override
  String get newConversation => 'Un nouveau chat';

  @override
  String get approveTitle => 'Mettre à jour la mise en page de connexion';

  @override
  String get approveNote => 'Exécuter des tests de projet';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => 'Organiser la documentation API';

  @override
  String get questionNote => 'En attente de réponse';

  @override
  String get question =>
      'Dans quelle langue la documentation doit-elle être rédigée?';

  @override
  String get optionChinese => 'Chinois';

  @override
  String get optionEnglish => 'English';

  @override
  String get reply => 'Réponse';

  @override
  String get approval => 'Approbation';

  @override
  String get viewRequest => 'Voir la demande';

  @override
  String get taskSearch => 'Améliorer la recherche de fichiers';

  @override
  String get taskSearchNote => 'Vérification de l\'index du répertoire';

  @override
  String get taskTest => 'Fixer la récupération de chat';

  @override
  String get taskTestNote => 'Tests en cours';

  @override
  String get taskDone => 'Mettre à jour le README du projet';

  @override
  String get taskDoneNote => '3 fichiers modifiés';

  @override
  String get ago => 'Juste à présent';

  @override
  String get minutesAgo => 'Il y a 12 minutes';

  @override
  String get allow => 'Autoriser une fois';

  @override
  String get deny => 'Refuser';

  @override
  String get approved => 'Autorisé · Échantillon';

  @override
  String get denied => 'Refusé · Échantillon';

  @override
  String get answered => 'Répondu · Échantillon';

  @override
  String get awaiting => 'En attente d\'approbation';

  @override
  String get working => 'En cours';

  @override
  String get viewChanges => 'Voir les changements';

  @override
  String get viewConversation => 'Voir le chat';

  @override
  String get chatTitle => 'Mettre à jour la mise en page de connexion';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => 'Aujourd\'hui 09:36';

  @override
  String get userMessage =>
      'Ajustez l\'espacement de connexion et unifiez les styles de saisie et de bouton tout en préservant la logique de connexion';

  @override
  String get assistantMessage =>
      'J\'ai vérifié la page de connexion et les composants de formulaire partagés, l\'espacement de saisie unifié et les styles de focus du clavier ajoutés';

  @override
  String get replyPreview => 'Exemple de flux de tâches';

  @override
  String get phaseThinking => 'Penser';

  @override
  String get phaseReading => 'Lecture de fichiers';

  @override
  String get phaseQuestion => 'En attente de réponse';

  @override
  String get phaseEditing => 'Édition de fichiers';

  @override
  String get phaseApproval => 'En attente d\'approbation';

  @override
  String get phaseTesting => 'Tests en cours';

  @override
  String get phaseReply => 'Réponse';

  @override
  String get phaseFollowup => 'Traitement de la file d\'attente';

  @override
  String get phaseComplete => 'Achevée';

  @override
  String get phaseFailed => 'Tests échoués';

  @override
  String get allowShort => 'Autoriser';

  @override
  String get queueShort => 'File d’attente';

  @override
  String get confirmShort => 'Confirmer';

  @override
  String get todoShort => 'À faire';

  @override
  String get todoInspect => 'Inspecter la page de connexion';

  @override
  String get todoEdit => 'Ajuster les styles de formulaire';

  @override
  String get todoTest => 'Exécuter des tests de projet';

  @override
  String get todoNarrow => 'Vérifiez l\'espacement étroit de l\'écran';

  @override
  String get workProcess => 'Activité';

  @override
  String workSteps(String count) {
    return '· $count étapes';
  }

  @override
  String get questionRecord => 'Confirmer la disposition';

  @override
  String get answerRecorded => 'Répondu';

  @override
  String get playFlow => 'Jouer la tâche';

  @override
  String get pauseFlow => 'Mettre la démo en pause';

  @override
  String get nextFlow => 'La prochaine étape';

  @override
  String get replyingNow => 'Réponse';

  @override
  String get toolReadLabel => 'Lire';

  @override
  String get toolEditLabel => 'Modifier';

  @override
  String get toolRunLabel => 'Exécuter';

  @override
  String get readGroup => '3 rangées';

  @override
  String get readFileResult => 'Fichier lu';

  @override
  String get readFileProgress => 'Lecture du fichier';

  @override
  String get flowAttachment =>
      'Mise à jour de la connexion: unifiez l\'espacement des formulaires, ajoutez des styles de focus du clavier et préservez la logique de connexion';

  @override
  String get readResult =>
      'Lire Login.tsx et les styles de formulaire partagés\nLa largeur du bouton mobile diffère du formulaire';

  @override
  String get layoutFindings =>
      'Le formulaire de connexion utilise l\'espacement du bureau et le bouton mobile ne remplit pas son conteneur';

  @override
  String get layoutQuestion =>
      'Le bouton de connexion mobile doit-il remplir la largeur?';

  @override
  String get questionPending => 'En attente de votre réponse';

  @override
  String get wideButton => 'Utiliser le bouton pleine largeur';

  @override
  String get keepButton => 'Conserver la largeur actuelle';

  @override
  String get editPlan =>
      'Je préserverai la logique de connexion, unifierai l\'espacement et ferai en sorte que le bouton mobile soit pleine largeur';

  @override
  String get editPlanKeep =>
      'Je garderai la largeur du bouton et la logique de connexion, en ajustant uniquement les styles d\'espacement et de mise au point';

  @override
  String get editThinking =>
      'Réutilisez les variables de style existantes et limitez les modifications de mise en page au formulaire de connexion';

  @override
  String get editResult =>
      'Mis à jour 3 fichiers\nAjout de styles de mise au point et de règles de mise en page mobile';

  @override
  String get beforeTest =>
      'Ensuite, je vais exécuter des tests de projet pour vérifier la présence de régressions';

  @override
  String get testTool => 'Exécuter des tests de projet';

  @override
  String get testProgress =>
      'Exécution de tests de formulaire de connexion…\nVérifier la mise au point et l\'interaction du clavier';

  @override
  String get testResult =>
      '12 tests réussis\nAucune régression logique de connexion trouvée';

  @override
  String get testFailure =>
      'Test d\'ordre de mise au point échoué\nFocus attendu sur le champ mot de passe, mais il est resté sur le champ nom d\'utilisateur';

  @override
  String get testFailed => 'Tests échoués';

  @override
  String get flowResult =>
      'Les styles d\'espacement et de mise au point de la connexion sont unifiés, avec un bouton mobile pleine largeur';

  @override
  String get queueSample =>
      'Vérifiez également l\'espacement des boutons sur les écrans étroits';

  @override
  String queueCount(String count) {
    return '$count messages en attente';
  }

  @override
  String queuePaused(String count) {
    return 'File d\'attente en pause · $count';
  }

  @override
  String get pauseQueue => 'Mettre la file en pause';

  @override
  String get resumeQueue => 'Reprendre la file d\'attente';

  @override
  String get sendNext => 'Envoyer la prochaine';

  @override
  String get enqueue => 'Ajouter à la liste d\'attente';

  @override
  String get queuedPreview => 'Ajouté à la file d\'attente d\'échantillons';

  @override
  String get moveUp => 'Déplacer vers le haut';

  @override
  String get followupThinking =>
      'Vérifiez les points d\'arrêt existants pour confirmer un espacement cohérent des boutons sur les écrans étroits';

  @override
  String get followupTool => 'Vérifiez les styles d\'écran étroit';

  @override
  String get followupToolResult =>
      '320px et 390px utilisent les mêmes règles d\'espacement';

  @override
  String get followupResult =>
      'L\'espacement des boutons à l\'écran étroit est cohérent; aucun autre changement n\'est nécessaire';

  @override
  String get deniedResult =>
      'Les tests n\'ont pas été exécutés; les modifications actuelles sont conservées';

  @override
  String get thinkingNow => 'Penser';

  @override
  String get toolsNow => 'Exécution en cours';

  @override
  String get toolPending => 'Non commencé';

  @override
  String get thoughtLive =>
      'Inspectez d\'abord la page de connexion et les composants du formulaire pour identifier les changements d\'espacement et de mise au point';

  @override
  String get toolsShort => '3 actions';

  @override
  String get thought => 'Raisonnement';

  @override
  String get thoughtContent =>
      'Réutilisez les composants de formulaire existants et ajustez uniquement la mise en page de connexion et les styles de mise au point';

  @override
  String get toolsComplete => '3 actions menées à bien';

  @override
  String get toolRead => 'Lire les composants de connexion et de formulaire';

  @override
  String get toolEdit =>
      'Mettre à jour les styles d’espacement et de mise au point';

  @override
  String get toolDiff => 'Inspecter les diffs de fichiers';

  @override
  String get changedFiles => '3 fichiers modifiés';

  @override
  String get approvalBody =>
      'Exécuter des tests dans l’arborescence sailry-web sur Studio';

  @override
  String get approvalResolved => 'Approbation résolue';

  @override
  String get chatContinue => 'Continuez à décrire votre tâche';

  @override
  String get describeTask => 'Décrivez votre tâche';

  @override
  String get send => 'Envoyer';

  @override
  String get attach => 'Joindre';

  @override
  String get voice => 'Entrée de voix';

  @override
  String get voiceNote => 'Cet aperçu n\'accède pas au microphone';

  @override
  String get attachmentNote => 'Exemple de pièce jointe ajouté';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => 'Supprimer la pièce jointe';

  @override
  String get sentPreview => 'Aperçu seulement, non envoyé';

  @override
  String get model => 'Modèle';

  @override
  String get modelSource => 'Configuration actuelle du chat · Studio';

  @override
  String get copy => 'Copier';

  @override
  String get copied => 'Copié';

  @override
  String get copyFailed =>
      'Copie indisponible; sélectionnez le texte manuellement';

  @override
  String get more => 'Plus';

  @override
  String get close => 'Fermer';

  @override
  String get back => 'Retour';

  @override
  String get cancel => 'Annuler';

  @override
  String get save => 'Enregistrer';

  @override
  String get select => 'Sélectionner';

  @override
  String get sessionActions => 'Actions de chat';

  @override
  String get queue => 'File de messages';

  @override
  String get queueEmpty => 'Pas de messages en file d\'attente';

  @override
  String get fork => 'Dupliquer la conversation';

  @override
  String get forked => 'Exemple de fourche créée';

  @override
  String get archive => 'Archiver le chat';

  @override
  String get archived => 'Archivé en prévisualisation';

  @override
  String get stop => 'Arrêter la tâche';

  @override
  String get stopped => 'Tâche arrêtée en prévisualisation';

  @override
  String get stoppedStatus => 'Arrêté';

  @override
  String get hostSubtitle => 'Vos nœuds d\'exécution';

  @override
  String get pair => 'Connecter l\'hôte';

  @override
  String get online => 'En ligne';

  @override
  String get offline => 'Hors ligne';

  @override
  String get connection => 'Connexion';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 couleurs';

  @override
  String get laptopSystem => 'Dernière connexion il y a 2 heures';

  @override
  String get statusHealthy => 'En bonne santé';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => 'Mémoire';

  @override
  String get disk => 'Disque';

  @override
  String get metrics => 'Utilisation des ressources';

  @override
  String get activity => 'Activité';

  @override
  String get lastHour => 'Dernières 60 minutes';

  @override
  String get sessionCount => 'Chats';

  @override
  String get terminalCount => 'Terminaux';

  @override
  String get projectCount => 'Projets';

  @override
  String get processes => 'Processus';

  @override
  String get process => 'Nom';

  @override
  String get network => 'Réseau';

  @override
  String get details => 'Détails';

  @override
  String get manageHost => 'Détails de l\'hôte';

  @override
  String get hostProjects => 'Projets d &apos; accueil';

  @override
  String get connectionDetails => 'Détails de connexion';

  @override
  String get direct => 'Direct';

  @override
  String get relay => 'Relais';

  @override
  String get latency => 'Latence';

  @override
  String get hostOffline => 'Hôte hors ligne; affichant son dernier état connu';

  @override
  String get retry => 'Réessayer';

  @override
  String get retryNote => 'L\'aperçu n\'est pas connecté à un hôte réel';

  @override
  String get pairTitle => 'Connecter un hôte';

  @override
  String get pairDescription =>
      'Entrez le code d\'appairage à 6 chiffres affiché sur l\'hôte';

  @override
  String get pairCode => 'Code d\'appairage';

  @override
  String get pairHint => 'Le code d\'appairage expire dans 60 secondes';

  @override
  String get pairDemo => 'Simuler la connexion';

  @override
  String get pairSuccess => 'Exemple d\'hôte ajouté';

  @override
  String get pairInvalid => 'Entrez 6 chiffres';

  @override
  String get workspace => 'Espace de travail';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => 'Choisir un hôte';

  @override
  String get selectProject => 'Choisir un projet';

  @override
  String get selectBranch => 'Choisissez l\'arbre de travail';

  @override
  String get mainBranch => 'Arbre de travail principal';

  @override
  String get featureBranch => 'Mise en page de connexion';

  @override
  String get connectionTools => 'Connexions';

  @override
  String get workspaceResources => 'Espace de travail';

  @override
  String get confirm => 'Confirmer';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => 'Changements, branches et histoire';

  @override
  String get gitBranches => 'Branches';

  @override
  String get gitHistory => 'Historique';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added lignes ajoutées · $removed supprimées';
  }

  @override
  String get gitActions => 'Actions Git';

  @override
  String get gitFetch => 'Fetch';

  @override
  String get gitPull => 'Pull';

  @override
  String get gitPush => 'Push';

  @override
  String get gitCurrent => 'Branche actuelle';

  @override
  String get gitCreateBranch => 'Nouvelle branche';

  @override
  String get gitBranchName => 'Nom de la branche';

  @override
  String get gitSwitch => 'Changer de branche';

  @override
  String get gitMerge => 'Branche de fusion';

  @override
  String get gitDeleteBranch => 'Supprimer la branche';

  @override
  String get gitHistoryLayout =>
      'Ajuster l\'espacement du formulaire de connexion';

  @override
  String get gitHistoryInit => 'Initialiser la page de login';

  @override
  String get gitPreview => 'Git simulation seulement; dépôt inchangé';

  @override
  String get gitDirty => 'Valider les modifications actuelles en premier';

  @override
  String get gitSwitchNote =>
      'Changer de branche dans cet arbre de travail; simulation uniquement';

  @override
  String get gitDeleteNote =>
      'Supprimer la branche sélectionnée; simulation seulement';

  @override
  String get gitInvalidBranch => 'Le nom ou la branche invalide existe déjà';

  @override
  String get review => 'Examiner';

  @override
  String get browseFiles => 'Parcourir le worktree';

  @override
  String get reviewFiles => 'Afficher les modifications de code';

  @override
  String get selectWorkspace => 'Projet et arbre de travail';

  @override
  String get resourceSummary => '2 conversations · 1 terminal';

  @override
  String get searchFiles => 'Recherche de fichiers';

  @override
  String get recentFiles => 'Fichiers';

  @override
  String get src => 'Source';

  @override
  String get folder => 'Dossier';

  @override
  String get modified => 'Modifié';

  @override
  String get filePreview => 'Aperçu du fichier';

  @override
  String get fileSample => 'Exemple de contenu de fichier';

  @override
  String get edit => 'Modifier';

  @override
  String get savePreview => 'Modifications enregistrées dans cet aperçu';

  @override
  String get unsaved => 'Non enregistré';

  @override
  String get discard => 'Ignorer les modifications';

  @override
  String get discardConfirm =>
      'Annuler les modifications non enregistrées à ce fichier?';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => 'Plus de ressources';

  @override
  String get ssh => 'SSH';

  @override
  String get database => 'Base de données';

  @override
  String get ports => 'Redirection de port';

  @override
  String get browser => 'Aperçu du Web';

  @override
  String get portsSub => '1 avant';

  @override
  String get connectionOwner => 'Execution Node · Appartement meublé';

  @override
  String get openTerminal => 'Ouvrir le terminal';

  @override
  String get tables => 'Tableaux';

  @override
  String get portNote => '· Pas d\'écouteur de port local';

  @override
  String get portTarget => 'Port cible';

  @override
  String get localPort => 'Port local';

  @override
  String get closePort => 'Fermer vers l\'avant';

  @override
  String get portClosed => 'Échantillon en avant fermé';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => 'Arbre de travail';

  @override
  String get staged => 'Indexé';

  @override
  String get diffSummary => '3 rangées';

  @override
  String get stage => 'Toutes les étapes';

  @override
  String get unstage => 'Désindexer';

  @override
  String get commit => 'Commit';

  @override
  String get commitTitle => 'Procéder à des modifications';

  @override
  String get commitMessage => 'Message de commit';

  @override
  String get commitPlaceholder => 'Décrivez les changements';

  @override
  String get commitPreview => 'Simuler un commit';

  @override
  String get committed => 'Exemple de commit terminé';

  @override
  String get noChanges => 'Aucun changement à valider';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => 'Continuer à modifier';

  @override
  String get diffSelection => 'Choisir le fichier modifié';

  @override
  String get terminalKeyboard => 'Clavier';

  @override
  String get terminalEnter => 'Entrée';

  @override
  String get terminalOutputLabel => 'Sortie du terminal';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => 'Lecture seule';

  @override
  String get takeControl => 'Prenez le contrôle';

  @override
  String get hasControl => 'Contrôle des entrées';

  @override
  String get releaseControl => 'Contrôle des rejets';

  @override
  String get terminalPlaceholder => 'Entrez un exemple de commande';

  @override
  String get terminalPreview => '· Les commandes ne sont pas exécutées';

  @override
  String get terminalOutput =>
      'Commande reçue en prévisualisation, non exécutée';

  @override
  String get terminalControlNote =>
      'Prendre le contrôle pour envoyer l\'entrée; simulé ici';

  @override
  String get usageSubtitle => 'Sailry chats seulement';

  @override
  String get week => 'Cette semaine';

  @override
  String get month => 'Ce mois-ci';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => 'Responses';

  @override
  String usageCoverage(String priced, String total) {
    return '$priced / $total réponses tarifées';
  }

  @override
  String get usageEmpty => 'Aucune donnée d\'utilisation';

  @override
  String get estimatedCost => 'Coût estimatif';

  @override
  String get costCoverage => '42 / 48 réponses tarifées';

  @override
  String get partial => 'Données partielles';

  @override
  String get sourcesPartial => '2 / 3 hôtes mis à jour';

  @override
  String get input => 'Entrée';

  @override
  String get output => 'Sortie';

  @override
  String get cached => 'Hits de cache';

  @override
  String get modelUsage => 'Distribution du modèle';

  @override
  String get hostUsage => 'Utilisation de l &apos; hôte';

  @override
  String get recentRequests => 'Réponses récentes';

  @override
  String get allUsage => 'Détails d\'utilisation';

  @override
  String get usageNote =>
      'Les coûts sont des estimations; certaines réponses ne sont pas chiffrées';

  @override
  String get sourceNote =>
      'Les hôtes hors ligne conservent leurs dernières données connues';

  @override
  String get profileSubtitle => 'Contrôleur mobile';

  @override
  String get localSettings => 'Les préférences locales';

  @override
  String get nodeSettings => 'Paramètres d\'exécution Node';

  @override
  String get appearance => 'Apparence';

  @override
  String get notifications => 'Notifications';

  @override
  String get enabled => 'Activé';

  @override
  String get disabled => 'Désactivé';

  @override
  String get add => 'Ajouter';

  @override
  String get configName => 'Nom';

  @override
  String get configEndpoint => 'Endpoint';

  @override
  String get configModels => 'Modèles';

  @override
  String get configInstructions => 'Instructions';

  @override
  String get configContent => 'Contenu';

  @override
  String get configEmpty => 'Aucune entrée';

  @override
  String get configDuplicate => 'Le nom existe déjà';

  @override
  String configDelete(String name) {
    return 'Supprimer «$name»?';
  }

  @override
  String get speechInput => 'Entrée de voix';

  @override
  String get developerInstructions =>
      'Modifier le code de la tâche et vérifier le résultat';

  @override
  String get reviewerInstructions =>
      'Examiner les modifications de code et identifier les problèmes';

  @override
  String get projectConventions => 'Conventions de projet';

  @override
  String get memoryContent => 'Préserver le style de code existant';

  @override
  String get providers => 'Modèles et fournisseurs';

  @override
  String get roles => 'Rôles';

  @override
  String get memorySettings => 'Mémoire';

  @override
  String get speech => 'Discours';

  @override
  String nodeSettingsNote(String host) {
    return 'Configuration enregistrée sur $host';
  }

  @override
  String get about => 'À propos de Sailry';

  @override
  String get aboutBody =>
      'Aperçu de l\'interaction mobile, non connecté aux services';

  @override
  String get settingsSaved => 'Paramètres mis à jour dans cet aperçu';

  @override
  String get modelPicker => 'Choisir un modèle';

  @override
  String get nodeDefaults => 'Node par défaut';

  @override
  String get providerNote =>
      '· Les informations d\'identification restent sur l\'exécution Node';

  @override
  String get roleNote => 'Exemple de rôle · S\'applique aux nouveaux chats';

  @override
  String get auto => 'Automatique';

  @override
  String get manual => 'Demandez à chaque fois';

  @override
  String get notificationsNote =>
      'Contrôle uniquement les notifications d\'aperçu';

  @override
  String get memoryNote => 'Échantillon Node mémoire';

  @override
  String get speechNote => 'Utilise la configuration d\'exécution Node';

  @override
  String get newTaskHost => 'Exécution host';

  @override
  String get newTaskProject => 'Projet';

  @override
  String get newTaskWorktree => 'Arbre de travail';

  @override
  String get create => 'Créer';

  @override
  String get taskCreated => 'Exemple de chat créé';

  @override
  String get required => 'Décrivez votre tâche en premier';

  @override
  String get notificationsEmpty => 'Pas de nouvelles notifications';

  @override
  String get reviewTitle => 'Références de conception';

  @override
  String get reviewIntro =>
      'Les pages suivent la source courante; cet aperçu n\'établit pas l\'acceptation du service mobile';

  @override
  String get reviewConversation =>
      'Chats, approbations, questions et file d\'attente';

  @override
  String get reviewConversationText =>
      'Les tâches conservent la propriété de l\'hôte, du projet et de l\'arborescence de travail ; élargissez les enregistrements d\'outils et les approbations dans les chats';

  @override
  String get reviewResources => 'Limes, Git, bornes et connexions';

  @override
  String get reviewResourcesText =>
      'L\'édition de fichiers, la mise en attente, les commits et les portages conservent leurs points d\'entrée; les détails s\'ouvrent sur des pages secondaires';

  @override
  String get reviewHosts => 'Connexions et surveillance des hôtes';

  @override
  String get reviewHostsText =>
      'Nœuds appariés, codes à 6 chiffres, utilisation des ressources et processus; l\'état hors ligne n\'est pas affiché comme actif';

  @override
  String get reviewUsage => 'Utilisation et Node paramètres';

  @override
  String get reviewUsageText =>
      'Sailry clavardage seulement; l\'utilisation agrégée conserve l\'exhaustivité et les coûts indiquent les estimations et la couverture';

  @override
  String get reviewBoundary => 'Frontière mobile';

  @override
  String get reviewBoundaryText =>
      'Le pont mobile expose les connexions, les chats, les terminaux et l\'utilisation; cet aperçu commence par n ° Node, modèles, jumelage, terminaux ou plugins';

  @override
  String get reviewVisual => 'Références visuelles';

  @override
  String get reviewVisualText =>
      'Référence 1: hiérarchie du chat; référence 2: cartes souples et navigation flottante; référence 3: surveillance compacte';

  @override
  String get hostConnectPrompt => 'Connecter un hôte';

  @override
  String get hostDisconnected => 'Connexion perdue';

  @override
  String get language => 'Langue';

  @override
  String get languageSystem => 'Système';

  @override
  String get languageChinese => '简体中文';

  @override
  String get languageEnglish => 'English';

  @override
  String get languageTraditionalChinese => '繁體中文';

  @override
  String get languageJapanese => '日本語';

  @override
  String get languageKorean => '한국어';

  @override
  String get languageFrench => 'Français';

  @override
  String get languageGerman => 'Deutsch';

  @override
  String get languageSpanish => 'Español';

  @override
  String get languagePortugueseBrazil => 'Português (Brasil)';

  @override
  String get languageRussian => 'Русский';

  @override
  String get backgroundConnection => 'Restez connecté en arrière-plan';

  @override
  String get backgroundConnectionActive =>
      'Maintenir les connexions hôte actives';

  @override
  String get backgroundConnectionFailed =>
      'Connexion en arrière-plan non activée; réessayez';

  @override
  String get resetReasoning => 'Réinitialiser l\'effort';

  @override
  String get completionAlerts => 'Alertes d\'achèvement';

  @override
  String get notificationsReadAll => 'Marquer tout comme lu';

  @override
  String get notificationsOpen => 'Ouvrir';

  @override
  String get preferencesFailed => 'Préférences non sauvegardées; réessayez';

  @override
  String get connectFirst => 'Connectez un hôte pour commencer';

  @override
  String get initializing => 'Démarrage en cours';

  @override
  String get startupFailed => 'Le démarrage a échoué';

  @override
  String get retryConnection => 'Réessayer';

  @override
  String get pairAction => 'Connecter';

  @override
  String get pairFailed => 'Connexion échouée; réessayez';

  @override
  String get pairExpired => 'Code d\'appairage expiré; obtenir un nouveau code';

  @override
  String get pairing => 'Connexion en cours';

  @override
  String get hostUnavailable => 'Hébergeur non connecté';

  @override
  String get hostMetricsFailed => 'Impossible de lire l\'état de l\'hôte';

  @override
  String get hostProcessesEmpty => 'Pas de processus';

  @override
  String get hostRegisterProject => 'Ajouter un projet';

  @override
  String get hostChooseDirectory => 'Choisir un répertoire';

  @override
  String get hostChooseFile => 'Choisir un fichier';

  @override
  String get hostParentDirectory => 'Répertoire parent';

  @override
  String get hostEmptyDirectory => 'Le répertoire est vide';

  @override
  String get hostLoadMore => 'Charger plus d\'articles';

  @override
  String get hostProjectName => 'Nom du projet';

  @override
  String get hostProjectPath => 'Chemin du projet sur l\'hôte';

  @override
  String get hostProjectFailed => 'Impossible d\'ajouter un projet';

  @override
  String get hostUnknown => 'Aucune donnée';

  @override
  String get hostRefresh => 'Actualiser';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => 'Mémoire';

  @override
  String get hostMetricDisk => 'Disque';

  @override
  String get failureConflict => 'Contenu modifié; recharger et réessayer';

  @override
  String get failureUnknown =>
      'Résultat non confirmé; vérifiez d\'abord l\'état de l\'hôte';

  @override
  String get failureDenied => 'Permission refusée';

  @override
  String get failureUnavailable => 'Hébergeur non connecté';

  @override
  String get failureBusy => 'Service occupé; réessayez plus tard';

  @override
  String get failureGeneric => 'L\'opération a échoué';

  @override
  String get settingsSpeechLanguage => 'Langue';

  @override
  String get settingsSpeechAuto => 'Détecter automatiquement';

  @override
  String get settingsSpeechChinese => 'Chinois';

  @override
  String get settingsSpeechEnglish => 'English';

  @override
  String get settingsSpeechReady => 'Modèle de discours prêt';

  @override
  String get settingsSpeechDownload => 'Télécharger le modèle de discours';

  @override
  String get settingsSpeechFailed => 'Modèle de discours non prêt; réessayez';

  @override
  String get settingsNoHost => 'Connecter un hôte en premier';

  @override
  String get settingsUnavailable => 'Non disponible';

  @override
  String get settingsLoadFailed => 'Impossible de charger';

  @override
  String get settingsSaveFailed =>
      'Échec de l\'enregistrement; brouillon conservé';

  @override
  String get settingsConflict =>
      'Les paramètres ont été modifiés; rouvrez et réessayez';

  @override
  String get settingsUnknown =>
      'Résultat non confirmé; actualiser pour vérifier';

  @override
  String get settingsRetry => 'Réessayer';

  @override
  String get settingsLoading => 'Chargement';

  @override
  String get settingsRequired => 'Entrez une valeur';

  @override
  String get settingsKey => 'ID';

  @override
  String get settingsDescription => 'Description';

  @override
  String get settingsInstructions => 'Instructions';

  @override
  String get settingsModels => 'Identifiant du modèle, un par ligne';

  @override
  String get settingsApi => 'Format API';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'Clé API';

  @override
  String get settingsEnabled => 'Activé';

  @override
  String get settingsDriver => 'Service';

  @override
  String get settingsModel => 'Modèle';

  @override
  String get settingsMemoryAuto => 'Enregistrer automatiquement';

  @override
  String get settingsMemoryBudget => 'Octets de contexte';

  @override
  String get settingsMemoryReview => 'Intervalle d &apos; examen (jours)';

  @override
  String get settingsMemoryRecords => 'Entrées de mémoire';

  @override
  String get settingsMemoryKind => 'Type';

  @override
  String get settingsMemoryUser => 'Utilisateur';

  @override
  String get settingsMemoryFeedback => 'Commentaires';

  @override
  String get settingsMemoryProject => 'Projet';

  @override
  String get settingsMemoryReference => 'Référence';

  @override
  String get settingsArchived => 'Archivé';

  @override
  String get settingsEmpty => 'Aucun enregistrement';

  @override
  String get settingsUsageUnknown => 'Inconnu';

  @override
  String get settingsUsagePartial => 'Certains hôtes sont indisponibles';

  @override
  String get settingsUsageCache => 'En cache';

  @override
  String get settingsUsageInput => 'Entrée non mise en cache';

  @override
  String get settingsUsageOutput => 'Sortie';

  @override
  String get settingsUsageDaily => 'Quotidien';

  @override
  String get settingsUsageWeekly => 'Hebdomadaire';

  @override
  String get settingsUtc => 'UTC';
}
