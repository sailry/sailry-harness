// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for French (`fr`).
class AppLocalizationsFr extends AppLocalizations {
  AppLocalizationsFr([String locale = 'fr']) : super(locale);

  @override
  String get guidanceWithdraw => 'Retirer la consigne';

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
  String get conversationStepSkipped => 'Ignoré';

  @override
  String get conversationChild => 'Sous-tâche';

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
  String get conversationContinued => 'Poursuivi';

  @override
  String get conversationFailedStatus => 'Échec';

  @override
  String get conversationStopping => 'Arrêt en cours';

  @override
  String get conversationQueued => 'En file d\'attente';

  @override
  String get conversationGenerating => 'Réponse';

  @override
  String get conversationWaiting => 'En attente de confirmation';

  @override
  String get conversationWaitingChild => 'En attente d\'un sous-agent';

  @override
  String conversationWaitingChildren(String count) {
    return 'En attente de $count sous-agents';
  }

  @override
  String get conversationCompacting => 'Compacter le contexte';

  @override
  String get conversationCompacted => 'Contexte compacté';

  @override
  String get conversationToolWaiting => 'En attente';

  @override
  String get conversationStartCoding => 'Démarrer l\'exécution';

  @override
  String get conversationPlanFeedback => 'Suggérer des changements';

  @override
  String get conversationOther => 'Autre';

  @override
  String get conversationSubmit => 'Envoyer';

  @override
  String get conversationMode => 'Mode de travail';

  @override
  String get conversationCode => 'Standard';

  @override
  String get conversationPlan => 'Plan';

  @override
  String get conversationFlagship => 'Phare';

  @override
  String get conversationAsk => 'Demandez à chaque fois';

  @override
  String get conversationProject => 'Accès au projet';

  @override
  String get conversationFull => 'Un accès complet';

  @override
  String get conversationBudget => 'Raisonnement budgétaire';

  @override
  String get conversationAttachment => 'Pièce jointe';

  @override
  String get conversationImageFailed => 'Impossible d\'afficher l\'image';

  @override
  String get conversationImageUnavailable => 'Image indisponible';

  @override
  String get conversationDownloadFailed =>
      'Impossible de charger la pièce jointe';

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
  String get turnChanges => 'Tourner les changements';

  @override
  String turnChangesCount(String count) {
    return '$count fichiers';
  }

  @override
  String get fileChangesApprovalOne => 'Appliquer le correctif à 1 fichier';

  @override
  String fileChangesApproval(String count) {
    return 'Appliquer le correctif à $count fichiers';
  }

  @override
  String get fileChangesAdded => 'Ajouté';

  @override
  String get fileChangesModified => 'Modifié';

  @override
  String get fileChangesRenamed => 'Renommé';

  @override
  String get fileChangesDeleted => 'Supprimé';

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
      'Garder ce tour et supprimer les messages suivants ? Les fichiers restent inchangés';

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
  String get resourceTerminalEnded => 'Terminaux fermés';

  @override
  String get resourceTerminalConnecting => 'Terminal de raccordement';

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
  String get unassignedChats => 'Chats';

  @override
  String get allWorktrees => 'Tous les arbres de travail';

  @override
  String get tasksActive => 'En cours';

  @override
  String get tasksToday => 'Aujourd’hui';

  @override
  String get yesterday => 'Hier';

  @override
  String get tasksThisWeek => 'Cette semaine';

  @override
  String get tasksEarlier => 'Plus tôt';

  @override
  String get filterProjects => 'Filtrer les projets';

  @override
  String get archiveShort => 'Archives';

  @override
  String get archiveTab => 'Archivé';

  @override
  String get delete => 'Supprimer';

  @override
  String get deleteTask => 'Supprimer le chat';

  @override
  String get deleteWarning =>
      'Ce chat ne peut pas être repris après la suppression';

  @override
  String get restored => 'Restauré à la maison';

  @override
  String get restore => 'Restaurer';

  @override
  String get archiveEmpty => 'Aucun chat archivé';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => 'Espace de travail mobile';

  @override
  String get preview => 'Aperçu';

  @override
  String get mixed => 'Lumière et obscurité';

  @override
  String get dark => 'Sombre';

  @override
  String get light => 'Clair';

  @override
  String get focus => 'Un seul écran';

  @override
  String get page => 'Choisir une page';

  @override
  String get design => 'Caractéristiques et design';

  @override
  String get footer => 'VAILLY / MOBILE';

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
  String get allHosts => 'Tous les hôtes';

  @override
  String get all => 'Tout';

  @override
  String get running => 'En cours';

  @override
  String get waiting => 'En attente';

  @override
  String get completed => 'Achevée';

  @override
  String get search => 'Rechercher';

  @override
  String get searchTasks => 'Rechercher des tâches et des projets';

  @override
  String get noResults => 'Aucune tâche correspondante';

  @override
  String get notification => 'Notifications';

  @override
  String get newConversation => 'Un nouveau chat';

  @override
  String get question =>
      'Dans quelle langue la documentation doit-elle être rédigée?';

  @override
  String get reply => 'Réponse';

  @override
  String get approval => 'Approbation';

  @override
  String get allow => 'Autoriser une fois';

  @override
  String get deny => 'Refuser';

  @override
  String get working => 'En cours';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => 'Aujourd\'hui 09:36';

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
  String get questionPending => 'En attente de votre réponse';

  @override
  String queueCount(String count) {
    return '$count messages en attente';
  }

  @override
  String queuePaused(String count) {
    return 'File d\'attente en pause · $count';
  }

  @override
  String queueGuidance(String queue) {
    return '$queue, consigne en attente d\'intégration';
  }

  @override
  String get pauseQueue => 'Mettre la file en pause';

  @override
  String get resumeQueue => 'Reprendre la file d\'attente';

  @override
  String get moveUp => 'Déplacer vers le haut';

  @override
  String get thinkingNow => 'Penser';

  @override
  String get thought => 'Raisonnement';

  @override
  String get describeTask => 'Décrivez votre tâche';

  @override
  String get send => 'Envoyer';

  @override
  String get attach => 'Joindre';

  @override
  String get voice => 'Entrée de voix';

  @override
  String get attachment => 'Pièce jointe';

  @override
  String get removeAttachment => 'Supprimer la pièce jointe';

  @override
  String get model => 'Modèle';

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
  String get queue => 'File de messages';

  @override
  String get queueEmpty => 'Pas de messages en file d\'attente';

  @override
  String get fork => 'Dupliquer la conversation';

  @override
  String get stop => 'Arrêter la tâche';

  @override
  String get stoppedStatus => 'Arrêté';

  @override
  String get pair => 'Connecter l\'hôte';

  @override
  String get online => 'En ligne';

  @override
  String get offline => 'Hors ligne';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => 'Mémoire';

  @override
  String get metrics => 'Utilisation des ressources';

  @override
  String get activity => 'Activité';

  @override
  String get processes => 'Processus';

  @override
  String get network => 'Réseau';

  @override
  String get relay => 'Relais';

  @override
  String get retry => 'Réessayer';

  @override
  String get pairTitle => 'Connecter un hôte';

  @override
  String get pairCode => 'Code d\'appairage';

  @override
  String get pairInvalid => 'Entrez 6 chiffres';

  @override
  String get workspace => 'Espace de travail';

  @override
  String get selectHost => 'Choisir un hôte';

  @override
  String get confirm => 'Confirmer';

  @override
  String get git => 'Git';

  @override
  String get gitBranches => 'Branches';

  @override
  String get gitHistory => 'Historique';

  @override
  String gitViewAll(String count) {
    return 'Voir les $count modifications';
  }

  @override
  String terminalRunningCount(String count) {
    return '$count en cours';
  }

  @override
  String get gitUnstaged => 'Non indexé';

  @override
  String get gitStageFile => 'Indexer';

  @override
  String gitCommitCount(String count) {
    return 'Valider $count';
  }

  @override
  String get gitRemote => 'Distant';

  @override
  String get gitCurrentShort => 'Actuelle';

  @override
  String gitDiscardConfirm(String name) {
    return 'Annuler toutes les modifications de « $name » ?';
  }

  @override
  String gitTrashConfirm(String name) {
    return 'Placer « $name » dans la corbeille de l’hôte ?';
  }

  @override
  String get gitCommitOptions => 'Options de validation';

  @override
  String get gitSkipHooks => 'Ignorer les hooks';

  @override
  String get gitPushAfterCommit => 'Pousser après validation';

  @override
  String get fileShare => 'Partager';

  @override
  String get fileTrash => 'Placer dans la corbeille';

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
  String get review => 'Examiner';

  @override
  String get selectWorkspace => 'Projet et arbre de travail';

  @override
  String get searchFiles => 'Recherche de fichiers';

  @override
  String get src => 'Source';

  @override
  String get folder => 'Dossier';

  @override
  String get modified => 'Modifié';

  @override
  String get edit => 'Modifier';

  @override
  String get unsaved => 'Non enregistré';

  @override
  String get discard => 'Ignorer les modifications';

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
  String get openFile => 'Ouvrir le fichier';

  @override
  String get closePort => 'Fermer vers l\'avant';

  @override
  String get workingTree => 'Arbre de travail';

  @override
  String get staged => 'Indexé';

  @override
  String get stage => 'Toutes les étapes';

  @override
  String get unstage => 'Désindexer';

  @override
  String get commit => 'Commit';

  @override
  String get commitMessage => 'Message de commit';

  @override
  String get noChanges => 'Aucun changement à valider';

  @override
  String get terminalKeyboard => 'Clavier';

  @override
  String get terminalEnter => 'Entrée';

  @override
  String get readOnly => 'Lecture seule';

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
  String get estimatedCost => 'Coût estimé (USD)';

  @override
  String get partial => 'Données partielles';

  @override
  String get input => 'Entrée';

  @override
  String get output => 'Sortie';

  @override
  String get cached => 'Hits de cache';

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
  String get providers => 'Modèles et fournisseurs';

  @override
  String get speech => 'Discours';

  @override
  String get about => 'À propos de Sailry';

  @override
  String get modelPicker => 'Choisir un modèle';

  @override
  String get team => 'Équipe';

  @override
  String get executionModel => 'Modèle seul';

  @override
  String get teamDefault => 'Équipe par défaut';

  @override
  String get teamLead => 'Responsable';

  @override
  String get memberReset => 'Réinitialiser le modèle';

  @override
  String get auto => 'Automatique';

  @override
  String get manual => 'Demandez à chaque fois';

  @override
  String get required => 'Décrivez votre tâche en premier';

  @override
  String get hostConnectPrompt => 'Connecter un hôte';

  @override
  String get hostDisconnected => 'Connexion perdue';

  @override
  String get hostUpdateRequired => 'Mettez à jour l’hôte pour vous connecter';

  @override
  String get appUpdateRequired => 'Mettez à jour cette app pour vous connecter';

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
  String get preferencesFailed => 'Préférences non sauvegardées; réessayez';

  @override
  String get startupFailed => 'Le démarrage a échoué';

  @override
  String get pairAction => 'Connecter';

  @override
  String get pairFailed => 'Connexion échouée; réessayez';

  @override
  String get pairExpired => 'Code d\'appairage expiré; obtenir un nouveau code';

  @override
  String get pairing => 'Connexion en cours';

  @override
  String get hostRemove => 'Retirer l’hôte';

  @override
  String hostRemoveConfirm(String name) {
    return 'Retirer « $name » de cet appareil ? Associez-le à nouveau pour vous reconnecter';
  }

  @override
  String get hostRemoved => 'Hôte retiré';

  @override
  String get hostRemoveFailed => 'Impossible de retirer l’hôte';

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
  String get settingsRequired => 'Entrez une valeur';

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
  String get apiChatCompletions => 'Chat Completions';

  @override
  String get apiResponses => 'Responses';

  @override
  String get apiAnthropic => 'Anthropic';

  @override
  String get apiGemini => 'Gemini';

  @override
  String get apiDeepSeek => 'DeepSeek';

  @override
  String get apiAzureOpenAi => 'Azure OpenAI';

  @override
  String get apiAzureAi => 'Azure AI';

  @override
  String get apiPlugin => 'Plug-in';

  @override
  String get providerNew => 'Nouveau service';

  @override
  String get providerEmpty => 'Aucun service';

  @override
  String get providerDesktopHint =>
      'Ajoutez les préréglages et connexions de compte sur Desktop ; leurs modèles restent modifiables ici';

  @override
  String get providerConnection => 'Connexion';

  @override
  String get providerKeyUnset => 'Non défini';

  @override
  String get providerShowKey => 'Afficher la clé';

  @override
  String get providerHideKey => 'Masquer la clé';

  @override
  String providerModelCount(String count) {
    return '$count modèles';
  }

  @override
  String get providerFetch => 'Récupérer';

  @override
  String get providerFetchTitle => 'Récupérer les modèles';

  @override
  String providerFetched(String count) {
    return '$count renvoyés';
  }

  @override
  String get providerSelectAll => 'Tout sélectionner';

  @override
  String get providerSelectNone => 'Tout désélectionner';

  @override
  String get providerSearch => 'Rechercher des modèles';

  @override
  String get providerAdded => 'Ajouté';

  @override
  String providerContext(String tokens) {
    return 'Contexte $tokens';
  }

  @override
  String providerAddCount(String count) {
    return 'Ajouter $count';
  }

  @override
  String get providerModelId => 'ID du modèle';

  @override
  String get providerModelExists => 'Modèle déjà ajouté';

  @override
  String get providerDisplayName => 'Nom affiché';

  @override
  String get providerDefault => 'Par défaut';

  @override
  String get providerBasic => 'Général';

  @override
  String get providerContextTitle => 'Contexte';

  @override
  String get providerContextLength => 'Longueur du contexte';

  @override
  String get providerMaxOutput => 'Sortie max.';

  @override
  String get providerCompaction => 'Compaction auto';

  @override
  String get providerCapabilities => 'Capacités';

  @override
  String get providerVision => 'Entrée image';

  @override
  String get providerTools => 'Appels d’outils';

  @override
  String get providerReasoning => 'Raisonnement';

  @override
  String get providerEfforts => 'Effort de raisonnement';

  @override
  String get providerEffortPlaceholder => 'Effort ou budget de tokens';

  @override
  String get providerEffortInvalid =>
      'Effort de raisonnement invalide ; vérifiez les noms, les doublons et la prise en charge de l’API';

  @override
  String get providerEffortDefault => 'Effort par défaut';

  @override
  String get providerEffortSetDefault => 'Définir par défaut';

  @override
  String get providerSetDefault => 'Définir comme modèle par défaut';

  @override
  String get providerIsDefault => 'Modèle par défaut';

  @override
  String get providerDeleteModel => 'Supprimer le modèle';

  @override
  String get providerDelete => 'Supprimer le service';

  @override
  String get providerNumberInvalid => 'Saisissez un entier positif';

  @override
  String get providerLimitsInvalid =>
      'La sortie max. ne peut pas dépasser le contexte';

  @override
  String get settingsUsageUnknown => 'Inconnu';

  @override
  String get settingsUsagePartial => 'Certains hôtes ne sont pas à jour';

  @override
  String get settingsUsageCache => 'En cache';

  @override
  String get settingsUsageInput => 'Hors cache';

  @override
  String get settingsUsageOutput => 'Sortie';

  @override
  String get usageRange24h => '24 heures';

  @override
  String get usageRange7d => '7 jours';

  @override
  String get usageRange30d => '30 jours';

  @override
  String get usageRange90d => '90 jours';

  @override
  String get usageRange1y => '1 an';

  @override
  String get usageReasoning => 'Raisonnement';

  @override
  String get usageHitRate => 'Taux de succès du cache';

  @override
  String get usageSpeed => 'Vitesse moyenne';

  @override
  String get usageSpeedUnit => 'tok/s';

  @override
  String get usageCostPartial => 'Partiel';

  @override
  String get usageTrend => 'Évolution des tokens';

  @override
  String get usageLocalTime => 'Heure locale';

  @override
  String usageActiveDays(String count) {
    return '$count jours actifs';
  }

  @override
  String get usageBreakdown => 'Répartition';

  @override
  String get usageProvider => 'Fournisseur';

  @override
  String get usageShowAll => 'Tout afficher';

  @override
  String get usageCollapse => 'Afficher moins';

  @override
  String get usageUnassigned => 'Non attribué';

  @override
  String usageHit(String value) {
    return 'Succès $value';
  }

  @override
  String usageResponseCount(String count) {
    return '$count réponses';
  }

  @override
  String get artifactContents => 'Sommaire';

  @override
  String get artifactZoomIn => 'Agrandir';

  @override
  String get artifactZoomOut => 'Réduire';

  @override
  String get artifactVersions => 'Versions';

  @override
  String get artifactFormRequired => 'Remplissez les champs obligatoires';

  @override
  String get artifactFormInvalid => 'Impossible d’envoyer les réponses';

  @override
  String get artifactRenderFailed => 'Impossible d’afficher le graphique';

  @override
  String artifactReply(String id, String revision, String title, String form) {
    return 'Réponse à « $title » v$revision ($id/$form)';
  }

  @override
  String get connectionRouteP2p => 'P2P';

  @override
  String get connectionRouteRelay => 'Relay';

  @override
  String get connectionRouteUnknown => '—';

  @override
  String get backgroundNoticeTitle => 'Résultats en arrière-plan';

  @override
  String get backgroundCompleted => 'Terminé';

  @override
  String get backgroundFailed => 'Échec';

  @override
  String get backgroundCancelled => 'Annulé';

  @override
  String get backgroundStopped => 'Arrêté';

  @override
  String get backgroundTimedOut => 'Délai dépassé';

  @override
  String get backgroundInterrupted => 'Interrompu';

  @override
  String get backgroundStalled => 'Aucune sortie récente';

  @override
  String get backgroundUnknown => 'Résultat incertain';

  @override
  String conversationSteps(String count) {
    return '$count étapes';
  }

  @override
  String get conversationStep => '1 étape';

  @override
  String get activityProgress => 'Progression';

  @override
  String get activityBackground => 'Tâches en arrière-plan';

  @override
  String get activityPorts => 'Ports';

  @override
  String get hostConnecting => 'Connexion';

  @override
  String get hostInfo => 'Infos de l’hôte';

  @override
  String get hostLatency => 'Latence';

  @override
  String hostLatencySummary(String average, String peak) {
    return 'Moy. $average  Max $peak';
  }

  @override
  String get hostSystem => 'Système';

  @override
  String get hostProcessor => 'Processeur';

  @override
  String hostCores(String count) {
    return '$count cœurs';
  }

  @override
  String hostLoad(String value) {
    return 'Charge $value';
  }

  @override
  String hostMemoryShare(String percent) {
    return '$percent utilisé';
  }

  @override
  String get hostUptime => 'Disponibilité';

  @override
  String hostNodeUptime(String duration) {
    return 'Service $duration';
  }

  @override
  String hostDays(String count) {
    return '$count jours';
  }

  @override
  String hostHours(String count) {
    return '$count heures';
  }

  @override
  String hostMinutes(String count) {
    return '$count minutes';
  }

  @override
  String get hostTrend => 'Tendance';

  @override
  String get hostTrendWindow => 'Dernière minute';

  @override
  String get hostChartStart => 'Il y a 60 s';

  @override
  String get hostChartNow => 'Maintenant';

  @override
  String get hostMetricGpu => 'GPU';

  @override
  String get hostMemoryUsed => 'Utilisée';

  @override
  String get hostMemoryCache => 'En cache';

  @override
  String get hostMemorySwap => 'Swap';

  @override
  String get hostGpuUsage => 'Utilisation';

  @override
  String get hostNetwork => 'Réseau';

  @override
  String hostNetworkTotals(String received, String sent) {
    return 'Total ↓ $received  ↑ $sent';
  }

  @override
  String get hostDownload => 'Réception';

  @override
  String get hostUpload => 'Envoi';

  @override
  String hostPartitions(String count) {
    return '$count partitions';
  }

  @override
  String get hostDiskRead => 'Lecture';

  @override
  String get hostDiskWrite => 'Écriture';

  @override
  String get hostProcesses => 'Processus';

  @override
  String get hostSortCpu => 'Par CPU';

  @override
  String get hostSortMemory => 'Par mémoire';

  @override
  String get hostSortRecent => 'Récents';

  @override
  String get hostSearchProcesses => 'Rechercher des processus';

  @override
  String get hostSearchProjects => 'Rechercher des projets';

  @override
  String hostProcessLimit(String count) {
    return '$count premiers affichés';
  }

  @override
  String get hostNoMatches => 'Aucun résultat';

  @override
  String get modelFeatureOff => 'Standard';

  @override
  String get modelFeatureFast => 'Rapide';

  @override
  String get modelFeatureSpeed => 'Vitesse';

  @override
  String get backgroundHeld => 'Résultat non remis';

  @override
  String get backgroundHoldLimit => 'Non remis ; limite de tours atteinte';

  @override
  String get backgroundHoldStopped => 'Non remis ; réponse arrêtée';

  @override
  String get backgroundHoldInterrupted => 'Non remis ; réponse interrompue';

  @override
  String get backgroundHoldFailed => 'Non remis ; échec de la réponse';

  @override
  String get continuationRefused => 'Non poursuivi';

  @override
  String get continuationPlan => 'Non poursuivi en mode Plan';

  @override
  String get continuationChild => 'Une sous-tâche n\'est pas poursuivie';

  @override
  String get continuationArchived => 'Un chat archivé n\'est pas poursuivi';

  @override
  String get continuationWorktree =>
      'Non poursuivi ; arbre de travail manquant';

  @override
  String get continuationQueuePaused =>
      'Non poursuivi ; file d\'attente en pause';

  @override
  String get continuationQueuedInput => 'Non poursuivi ; messages en attente';

  @override
  String get continuationRunning => 'Non poursuivi ; une réponse est en cours';

  @override
  String get continuationInteraction =>
      'Non poursuivi ; une demande vous attend';

  @override
  String get continuationLimit => 'Limite de tours automatiques atteinte';

  @override
  String continuationLimitCount(String count, String limit) {
    return 'Limite de tours automatiques atteinte · $count/$limit';
  }

  @override
  String get continuationStale => 'Non poursuivi ; le chat a avancé';

  @override
  String get conversationContextOverflow =>
      'Limite de contexte du modèle dépassée';

  @override
  String get conversationOutputLimit => 'Réponse coupée à la limite de sortie';
}
