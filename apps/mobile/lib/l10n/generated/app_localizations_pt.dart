// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Portuguese (`pt`).
class AppLocalizationsPt extends AppLocalizations {
  AppLocalizationsPt([String locale = 'pt']) : super(locale);

  @override
  String get updatesVersion => 'Versão';

  @override
  String get updatesCheck => 'Verifique se há atualizações';

  @override
  String get updatesChecking => 'Verificando';

  @override
  String updatesAvailable(String version) {
    return 'A versão $version está disponível';
  }

  @override
  String get updatesDownload => 'Baixar a atualização';

  @override
  String get updatesCurrent => 'Você está atualizado';

  @override
  String get updatesUnpublished => 'Nenhum lançamento móvel ainda';

  @override
  String get updatesCheckFailed => 'Não foi possível verificar atualizações';

  @override
  String get updatesOpenFailed => 'Não foi possível abrir o download';

  @override
  String get retryTask => 'Tentar novamente';

  @override
  String get welcomeTitle => 'O que você gostaria de trabalhar hoje?';

  @override
  String get welcomeExplore => 'Explorar um projeto';

  @override
  String get welcomeExploreDetail =>
      'Entenda sua estrutura e pontos de entrada';

  @override
  String get welcomeExplorePrompt =>
      'Ajude-me a entender este projeto, incluindo seus principais módulos e pontos de entrada.';

  @override
  String get welcomeBuild => 'Construa uma ideia';

  @override
  String get welcomeBuildDetail => 'Dê vida à sua ideia';

  @override
  String get welcomeBuildPrompt =>
      'Eu quero adicionar um recurso a este projeto, primeiro confirme os requisitos comigo e esboce um plano de implementação.';

  @override
  String get welcomeReview => 'Revise as alterações';

  @override
  String get welcomeReviewDetail =>
      'Verificar alterações e potenciais problemas';

  @override
  String get welcomeReviewPrompt =>
      'Revise as alterações atuais neste projeto, com foco em possíveis problemas e testes ausentes.';

  @override
  String get welcomePlan => 'Faça um plano';

  @override
  String get welcomePlanDetail => 'Esclarecer metas e etapas';

  @override
  String get welcomePlanPrompt =>
      'Ajude-me a criar um plano passo-a-passo para o próximo trabalho de desenvolvimento.';

  @override
  String get conversationEmpty => 'Descreva sua tarefa';

  @override
  String get conversationLoading => 'A carregar chat';

  @override
  String get conversationReconnecting => 'Reconectando';

  @override
  String get conversationErrorDetails => 'Ver razão';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return 'Voltando a tentar a solicitação do modelo $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return 'Pedido de modelo reiterado $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count chamadas de ferramentas';
  }

  @override
  String get conversationGoal => 'Objetivo';

  @override
  String get conversationGoalBlocked => 'Bloqueado';

  @override
  String conversationGoalBudget(String count) {
    return 'Orçamento $count tokens';
  }

  @override
  String get conversationStepSkipped => 'Ignorado';

  @override
  String get conversationChild => 'Subtarefa';

  @override
  String get conversationChildReadonly => 'Subtarefa de chat';

  @override
  String get conversationOffline => 'Conexão perdida';

  @override
  String get conversationUnavailable => 'Chat não disponível';

  @override
  String get conversationFailed => 'Operação falhou; tente novamente';

  @override
  String get conversationUnknown =>
      'Resultado não confirmado; verifique o chat antes de agir novamente';

  @override
  String get conversationCheckResult => 'Verifique o resultado';

  @override
  String get conversationConflict =>
      'Configuração alterada; reabrir antes de agir novamente';

  @override
  String get conversationOlder => 'Carregar mensagens mais antigas';

  @override
  String get conversationNew => 'Um novo chat';

  @override
  String get conversationNoHost => 'Conecte um host primeiro';

  @override
  String get conversationNoProject => 'Adicione um projeto no host primeiro';

  @override
  String get conversationNoModel => 'Configurar um modelo no host primeiro';

  @override
  String get conversationNoTasks => 'Ainda não há chats';

  @override
  String get conversationNoMessages => 'Nenhuma mensagem encontrada';

  @override
  String get conversationPreviewUnavailable => 'Mensagem não disponível';

  @override
  String get conversationInterrupted => 'Interrompido';

  @override
  String get conversationFailedStatus => 'Falhou';

  @override
  String get conversationStopping => 'Parando';

  @override
  String get conversationQueued => 'Em fila';

  @override
  String get conversationProcessing => 'Processamento';

  @override
  String get conversationUnsynced => 'Estado não sincronizado';

  @override
  String get conversationGenerating => 'Respondendo';

  @override
  String get conversationWaiting => 'Aguardando a confirmação';

  @override
  String get conversationCompacting => 'Compactando o contexto';

  @override
  String get conversationForkConfirm =>
      'Bifurcar um bate-papo a partir deste registro?';

  @override
  String get conversationCompacted => 'Contexto compactado';

  @override
  String get conversationToolWaiting => 'Pendente';

  @override
  String get conversationToolRunning => 'Em execução';

  @override
  String get conversationToolReturned => 'Retornado';

  @override
  String get conversationToolCancelled => 'Cancelado';

  @override
  String get conversationToolNotExecuted => 'Não executado';

  @override
  String get conversationToolInterrupted => 'Interrompido';

  @override
  String get conversationUnsupportedInput =>
      'Lidar com esta entrada no desktop';

  @override
  String get conversationStartCoding => 'Iniciar a execução';

  @override
  String get conversationPlanFeedback => 'Sugerir alterações';

  @override
  String get conversationOther => 'Outro';

  @override
  String get conversationSubmit => 'Enviar';

  @override
  String get conversationSource => 'Fonte';

  @override
  String get conversationMode => 'Modo de trabalho';

  @override
  String get conversationCode => 'Executar';

  @override
  String get conversationPlan => 'Plano';

  @override
  String get conversationPermission => 'Permissões';

  @override
  String get conversationAsk => 'Pergunte cada vez';

  @override
  String get conversationProject => 'Acesso ao projeto';

  @override
  String get conversationFull => 'Acesso total';

  @override
  String get conversationReasoning => 'Esforço de raciocínio';

  @override
  String get conversationDefault => 'Padrão';

  @override
  String get conversationNone => 'Desativado';

  @override
  String get conversationMinimal => 'Mínimo';

  @override
  String get conversationLow => 'Baixo';

  @override
  String get conversationMedium => 'Médio';

  @override
  String get conversationHigh => 'Alto';

  @override
  String get conversationXHigh => 'Maior';

  @override
  String get conversationMax => 'Máximo';

  @override
  String get conversationBudget => 'Raciocínio orçamental';

  @override
  String get conversationAttachment => 'Anexo';

  @override
  String get conversationAttachmentTooLarge =>
      'Anexo ilegível ou maior que 64 MB';

  @override
  String get conversationDownload => 'Ver anexo';

  @override
  String get conversationImageFailed => 'Não é possível exibir a imagem';

  @override
  String get conversationDownloadFailed => 'Não foi possível carregar o anexo';

  @override
  String get conversationReadonly => 'Este chat está arquivado';

  @override
  String get conversationMicrophoneDenied =>
      'Não é possível acessar o microfone';

  @override
  String get conversationRecordingFailed => 'O reconhecimento de fala falhou';

  @override
  String get conversationSpeechDisabled => 'A entrada de voz está desligada';

  @override
  String get conversationSpeechMissing =>
      'Baixe o modelo de fala em Configurações primeiro';

  @override
  String get conversationRecording => 'Gravação';

  @override
  String get conversationTranscribing => 'Transcrição';

  @override
  String get conversationRecordReady => 'Pronto para gravar';

  @override
  String get conversationStartRecording => 'Iniciar a gravação';

  @override
  String get conversationFinishRecording => 'Terminar a gravação';

  @override
  String get conversationSources => 'Fontes';

  @override
  String get conversationSearchSuggestions => 'Sugestões de pesquisa';

  @override
  String get conversationStats => 'Uso do chat';

  @override
  String get conversationStatsEmpty => 'Nenhum uso ainda';

  @override
  String get conversationStatsOverview => 'Visão geral';

  @override
  String get conversationStatsTokenGroup => 'Uso de token';

  @override
  String get conversationStatsCostGroup => 'Custo';

  @override
  String get conversationStatsGenerationGroup => 'Geração';

  @override
  String get conversationStatsTokens => 'Tokens';

  @override
  String get conversationStatsInput => 'Entrada';

  @override
  String get conversationStatsOutput => 'Saída';

  @override
  String get conversationStatsCached => 'Entrada em cache';

  @override
  String get conversationStatsReasoning => 'Raciocínio de saída';

  @override
  String get conversationStatsCacheRate => 'Hits de cache';

  @override
  String get conversationStatsCost => 'Custo estimado';

  @override
  String get conversationStatsCostCoverage => 'Cobertura de custos';

  @override
  String get conversationStatsSpeed => 'Velocidade de geração';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => 'Cobertura de tempo';

  @override
  String get conversationStatsTurns => 'Turnos';

  @override
  String get conversationStatsResponses => 'Respostas de modelo';

  @override
  String get conversationStatsContext => 'O contexto actual';

  @override
  String get conversationStatsInputCost => 'Custo de entrada';

  @override
  String get conversationStatsOutputCost => 'Custo de produção';

  @override
  String get conversationStatsCacheReadCost => 'Custo de leitura do cache';

  @override
  String get conversationStatsCacheWriteCost => 'Custo de gravação de cache';

  @override
  String get messageHistoryUpdated =>
      'Chat atualizado; registros originais mantidos';

  @override
  String get turnUndoUnsaved =>
      'Os arquivos têm alterações não salvas; salve ou descarte-as primeiro';

  @override
  String get codePlain => 'Texto simples';

  @override
  String get toolArguments => 'Argumentos';

  @override
  String get toolResult => 'Resultado';

  @override
  String get toolRaw => 'Resultado em bruto';

  @override
  String get turnChanges => 'Gire as mudanças';

  @override
  String turnChangesCount(String count) {
    return 'Arquivos $count';
  }

  @override
  String get turnUndo => 'Desfazer as alterações';

  @override
  String get turnUndoAll => 'Desfazer tudo';

  @override
  String get turnUndoConfirm =>
      'Conflitos com alterações posteriores irão parar a operação e o arquivo será desfeito';

  @override
  String get turnUndoDone => 'Desfeito';

  @override
  String get turnUndoPartial =>
      'Algumas alterações desfeitas; verifique os arquivos restantes';

  @override
  String get messageActions => 'Ações da mensagem';

  @override
  String get messageEdit => 'Editar e regenerar';

  @override
  String get messageEditConfirm =>
      'Substituir esta mensagem e o seguinte chat? Os arquivos não serão revertidos';

  @override
  String get messageRewind => 'Rebobine aqui';

  @override
  String get messageRewindConfirm =>
      'Os registros de bate-papo posteriores serão armazenados; os arquivos não serão revertidos';

  @override
  String get messageBackup => 'Ver chat backup';

  @override
  String get messageRegenerate => 'Gerar novamente';

  @override
  String get messageSearch => 'Pesquisar no chat';

  @override
  String get messageSearchHint => 'Pesquisar mensagens';

  @override
  String get messageSearchMissing =>
      'Esta mensagem não está mais no chat atual';

  @override
  String get messageSearchStale => 'O chat mudou; procure novamente';

  @override
  String get messageNoResults => 'Nenhuma mensagem correspondente';

  @override
  String get messageCheck => 'Verifique o resultado da operação';

  @override
  String get messageReference => 'Referência';

  @override
  String get messageReferenceContext =>
      'Esta referência pertence ao contexto em que a mensagem foi enviada';

  @override
  String get toolFailed => 'Falhou';

  @override
  String toolExitCode(String code) {
    return 'Código de saída $code';
  }

  @override
  String toolSignal(String signal) {
    return 'Terminado pelo sinal $signal';
  }

  @override
  String get toolTimedOut => 'Comando expirou';

  @override
  String get toolCancelled => 'Comando cancelado';

  @override
  String get toolOutcomeUnknown => 'Resultado do comando desconhecido';

  @override
  String get toolQuestionAnswered => 'Respondido';

  @override
  String get toolQuestionDeclined => 'Recusa';

  @override
  String get toolQuestionCancelled => 'Cancelado';

  @override
  String get fileLinkUnavailable => 'Não é possível abrir este link';

  @override
  String get imagePreview => 'Visualização da imagem';

  @override
  String get fileOpenExternal => 'Abrir com outro aplicativo';

  @override
  String get fileOpenFailed => 'Não é possível abrir o arquivo';

  @override
  String get fileNoApplication => 'Nenhum aplicativo pode abrir este arquivo';

  @override
  String get fileSaveBeforeShare => 'Salvar alterações antes de compartilhar?';

  @override
  String fileTrashConfirm(String name) {
    return 'Mover “$name” para a lixeira do host? Alterações não salvas também serão descartadas';
  }

  @override
  String get fileTrashUncertain =>
      'Resultado da exclusão não confirmado; tente novamente a consulta';

  @override
  String get fileSaveFailed => 'Salvar arquivo falhou';

  @override
  String get terminalHideKeyboard => 'Ocultar o teclado';

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
  String get terminalArrowLeft => 'Esquerda';

  @override
  String get terminalArrowUp => 'Acima';

  @override
  String get terminalArrowDown => 'Abaixo';

  @override
  String get terminalArrowRight => 'Direita';

  @override
  String get resourceNoWorkspace =>
      'Escolha uma árvore de trabalho em um host conectado';

  @override
  String get resourceDisconnected => 'Host não conectado';

  @override
  String get resourceRoot => 'Raiz';

  @override
  String get resourceMore => 'Carregar mais';

  @override
  String get resourcePartial => 'Conteúdo parcial mostrado';

  @override
  String get resourceEmpty => 'Nenhum conteúdo encontrado';

  @override
  String get resourceSaveError => 'Salvar falhou; rascunho mantido';

  @override
  String get resourceReloadConfirm =>
      'Descartar o rascunho e carregar o conteúdo mais recente?';

  @override
  String get resourceWorktreeCreate => 'Nova árvore de trabalho';

  @override
  String get resourceSessionServices => 'Serviços de chat';

  @override
  String get resourceServicesUnavailable => 'Lista de serviços indisponível';

  @override
  String get resourceNoServices => 'Nenhum endereço de serviço encontrado';

  @override
  String get resourceServiceOpen => 'Serviço aberto';

  @override
  String get resourceRemotePort => 'Porto remoto';

  @override
  String get resourceOpenPort => 'Porta de avanço';

  @override
  String get resourceOpenBrowser => 'Pré-visualizar página web';

  @override
  String get resourcePreviewFailed => 'Não foi possível carregar a página';

  @override
  String get resourcePreviewLink =>
      'Não é possível abrir este link na visualização';

  @override
  String get resourceForwardStopped => 'O encaminhamento foi interrompido';

  @override
  String get resourceTerminalControl => 'Assuma o controle';

  @override
  String get resourceTerminalControlHint => 'Controlado por outro dispositivo';

  @override
  String get resourceTerminalClaiming => 'Tomando o controle';

  @override
  String get resourceTerminalReadOnly => 'Read-only terminal';

  @override
  String get resourceTerminalEnded => 'O terminal terminou';

  @override
  String get resourceTerminalConnecting => 'Conectando o terminal';

  @override
  String get resourceTerminalInput => 'Entrada do terminal';

  @override
  String get resourceTerminalPaste => 'Colar';

  @override
  String get resourceGitNotRepository =>
      'Este diretório não é um repositório Git';

  @override
  String get resourceInvalidPort => 'Digite uma porta de 1–65535';

  @override
  String get tool_navigate => 'Abrir a página';

  @override
  String get tool_back => 'Ir para trás';

  @override
  String get tool_forward => 'Ir para a frente';

  @override
  String get tool_refresh => 'Atualizar a página';

  @override
  String get tool_right_click => 'Clique no elemento';

  @override
  String get tool_clear => 'Digite o texto';

  @override
  String get tool_select => 'Selecione uma opção';

  @override
  String get tool_hover => 'Elemento Hover';

  @override
  String get tool_scroll => 'Deslocar a página';

  @override
  String get tool_press_key => 'Pressione a tecla';

  @override
  String get tool_new_tab => 'Novo separador';

  @override
  String get tool_list_windows => 'Abas do navegador';

  @override
  String get tool_switch_window => 'Alternar a guia';

  @override
  String get tool_close_window => 'Fechar a janela';

  @override
  String get tool_close_session => 'Fechar o navegador';

  @override
  String get tool_screenshot => 'Captura de página';

  @override
  String get tool_print_to_pdf => 'Exportar para PDF';

  @override
  String get tool_file_upload => 'Enviar arquivo';

  @override
  String get tool_downloads => 'Ver os downloads';

  @override
  String get tool_save_download => 'Salvar arquivo baixado';

  @override
  String get tool_evaluate_js => 'Executar script de página';

  @override
  String get tool_get_cookies => 'Leia os cookies';

  @override
  String get tool_delete_all_cookies => 'Alterar os cookies';

  @override
  String get tool_drag_and_drop => 'Arrastar o elemento';

  @override
  String get tool_focus => 'Elemento de foco';

  @override
  String get tool_handle_alert => 'Alerta de página de manipulação';

  @override
  String get tool_database_catalog => 'Procurar na base de dados';

  @override
  String get tool_database_query => 'Base de dados de consulta';

  @override
  String get tool_database_execute => 'Executar operação de banco de dados';

  @override
  String get tool_search_memory => 'Pesquisar na memória';

  @override
  String get tool_review_memories => 'Revise as memórias';

  @override
  String get tool_consolidate_memories => 'Mesclar as memórias';

  @override
  String get tool_save_memory => 'Salvar a memória';

  @override
  String get tool_forget_memory => 'Apagar a memória';

  @override
  String get tool_update_plan => 'Plano de atualização';

  @override
  String get tool_create_goal => 'Criar um objetivo';

  @override
  String get tool_get_goal => 'Ver objetivo';

  @override
  String get tool_update_goal => 'Atualizar o objetivo';

  @override
  String get tool_spawn_agent => 'Subagente';

  @override
  String get tool_browser_tabs => 'Abas do navegador';

  @override
  String get tool_browser_read => 'Leia a página';

  @override
  String get tool_browser_navigate => 'Abrir a página';

  @override
  String get tool_browser_click => 'Clique no elemento';

  @override
  String get tool_browser_input => 'Digite o texto';

  @override
  String get tool_browser_scroll => 'Deslocar a página';

  @override
  String get tool_browser_back => 'Ir para trás';

  @override
  String get tool_browser_forward => 'Ir para a frente';

  @override
  String get tool_browser_refresh => 'Atualizar a página';

  @override
  String get tool_browser_open => 'Novo separador';

  @override
  String get tool_browser_close => 'Fechar a aba';

  @override
  String get tool_browser_focus => 'Alternar a guia';

  @override
  String get tool_browser_select => 'Selecione uma opção';

  @override
  String get tool_browser_hover => 'Elemento Hover';

  @override
  String get tool_browser_key => 'Pressione a tecla';

  @override
  String get tool_browser_frame => 'Quadro do interruptor';

  @override
  String get tool_browser_wait => 'Aguarde por página';

  @override
  String get tool_browser_screenshot => 'Captura de página';

  @override
  String get tool_ssh_run => 'Execute o comando SSH';

  @override
  String get tool_ssh_transfer => 'Transferir arquivo SSH';

  @override
  String get tool_list_worktrees => 'Lista de árvores de trabalho';

  @override
  String get tool_create_worktree => 'Nova árvore de trabalho';

  @override
  String get tool_register_worktree => 'Adicionar árvore de trabalho';

  @override
  String get tool_remove_worktree => 'Remover worktree';

  @override
  String get tool_google_search => 'Pesquisar na web';

  @override
  String get tool_web_fetch => 'Obter a página';

  @override
  String get tool_fetch_url => 'Obter a página';

  @override
  String get tool_read_file => 'Ler arquivo';

  @override
  String get tool_write_file => 'Gravar arquivo';

  @override
  String get tool_list_directory => 'Procurar no diretório';

  @override
  String get tool_search_files => 'Pesquisar arquivos';

  @override
  String get tool_run_command => 'Executar o comando';

  @override
  String get tool_read_command => 'Comando View background';

  @override
  String get tool_stop_command => 'Comando Stop';

  @override
  String get tool_load_skill => 'Habilidade de carga';

  @override
  String get tool_read_skill_resource => 'Ler recurso de habilidade';

  @override
  String get tool_computer_desktop => 'Ver área de trabalho';

  @override
  String get tool_computer_observe => 'Observe a tela';

  @override
  String get tool_computer_input => 'Computador de controle';

  @override
  String get tool_computer_focus => 'Aplicação Switch';

  @override
  String get tool_computer_open => 'Abrir o app';

  @override
  String get tool_git_status => 'Git status';

  @override
  String get tool_git_diff => 'Ver diferenças';

  @override
  String get tool_git_log => 'Git log';

  @override
  String get tool_inspect_image => 'Inspecionar a imagem';

  @override
  String get tool_generate_image => 'Gerar imagem';

  @override
  String get tool_generate_video => 'Gerar um vídeo';

  @override
  String get terminalUnavailable => 'O terminal não está conectado';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => 'Atualizar';

  @override
  String get loading => 'Carregando';

  @override
  String get home => 'Conversas';

  @override
  String get idle => 'Inativo';

  @override
  String get allProjects => 'Ver todos os projetos';

  @override
  String get allWorktrees => 'Todas as árvores de trabalho';

  @override
  String get filterProjects => 'Filtrar os projetos';

  @override
  String get closeSearch => 'Fechar a pesquisa';

  @override
  String onlineHostCount(String count) {
    return '$count em linha';
  }

  @override
  String get taskActions => 'Ações da tarefa';

  @override
  String get archiveShort => 'Arquivo';

  @override
  String get archiveTab => 'Arquivado';

  @override
  String get archivedTasks => 'Arquivado';

  @override
  String get delete => 'Excluir';

  @override
  String get deleteTask => 'Excluir o chat';

  @override
  String get deleteWarning => 'Este chat não pode ser retomado após a exclusão';

  @override
  String get busyDelete => 'Parar a tarefa antes de excluir este chat';

  @override
  String get stopBeforeDelete => 'Parar a tarefa';

  @override
  String get deleted => 'Bate-papo removido da visualização';

  @override
  String get restored => 'Restaurado para casa';

  @override
  String get restore => 'Restaurar';

  @override
  String get archiveEmpty => 'Sem chats arquivados';

  @override
  String get archiveKeepsRunning =>
      'O arquivamento não pára de executar tarefas';

  @override
  String get title => 'Sailry · Visualização móvel';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => 'Espaço de trabalho móvel';

  @override
  String get edition => 'EXPLORAÇÃO MÓVEL / 01';

  @override
  String get intro => 'Tarefas, chats e espaços de trabalho remotos';

  @override
  String get preview => 'Pré- visualização';

  @override
  String get sample =>
      'Dados de exemplo · As alterações permanecem nesta página';

  @override
  String get mixed => 'Luz e escuridão';

  @override
  String get dark => 'Escuro';

  @override
  String get light => 'Claro';

  @override
  String get gallery => 'Visão geral';

  @override
  String get focus => 'Tela única';

  @override
  String get reset => 'Redefinir a visualização';

  @override
  String get page => 'Escolha a página';

  @override
  String get experience => 'Abrir a página';

  @override
  String get backGallery => 'Voltar à visão geral';

  @override
  String get design => 'Características e design';

  @override
  String get footer => 'VAILRY / MÓVEL';

  @override
  String get footerNote => 'Visualização HTML local · Sem conexão de serviço';

  @override
  String get tasks => 'Tarefas';

  @override
  String get chat => 'Chat';

  @override
  String get hosts => 'Hosts';

  @override
  String get resources => 'Recursos';

  @override
  String get settings => 'Configurações';

  @override
  String get usage => 'Uso';

  @override
  String get changes => 'Alterações';

  @override
  String get terminal => 'Terminal';

  @override
  String get newTerminal => 'O novo terminal';

  @override
  String get files => 'Arquivos';

  @override
  String get project => 'Projeto';

  @override
  String get worktree => 'Árvore de trabalho';

  @override
  String get subtitleTasks =>
      'Tarefas entre hosts · Aprovações e respostas primeiro';

  @override
  String get subtitleChat =>
      'Conversa contínua · Expanda a atividade da ferramenta conforme necessário';

  @override
  String get subtitleHosts => 'Conexões, recursos de host e processos';

  @override
  String get subtitleResources => 'Host → Projeto → Árvore de trabalho';

  @override
  String get subtitleChanges => 'Diferenças de arquivos, preparação e commits';

  @override
  String get subtitleTerminal =>
      'Terminal remoto · Controle de entrada explícito';

  @override
  String get subtitleUsage => 'Sailry chats · Agregado entre hosts';

  @override
  String get subtitleSettings =>
      'Preferências locais e configurações de execução Node';

  @override
  String get allHosts => 'Todos os hosts';

  @override
  String get connectedHosts => '2 em linha';

  @override
  String get all => 'Tudo';

  @override
  String get running => 'Em execução';

  @override
  String get waiting => 'Pendente';

  @override
  String get completed => 'Concluído';

  @override
  String get taskProgress => 'Tarefa atual';

  @override
  String get taskWait => 'Aguardando sua decisão';

  @override
  String get taskRecent => 'Recentemente concluído';

  @override
  String get search => 'Pesquisar';

  @override
  String get searchTasks => 'Pesquisar tarefas e projetos';

  @override
  String get filterTasks => 'Filtrar as tarefas';

  @override
  String get noResults => 'Nenhuma tarefa correspondente';

  @override
  String get notification => 'Notificações';

  @override
  String get newTask => 'Nova tarefa';

  @override
  String get newConversation => 'Um novo chat';

  @override
  String get approveTitle => 'Atualizar o layout de login';

  @override
  String get approveNote => 'Executar testes de projeto';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => 'Organize API documentação';

  @override
  String get questionNote => 'Aguardando uma resposta';

  @override
  String get question => 'Qual idioma a documentação deve usar?';

  @override
  String get optionChinese => 'Chinês';

  @override
  String get optionEnglish => 'English';

  @override
  String get reply => 'Responder';

  @override
  String get approval => 'Homologação';

  @override
  String get viewRequest => 'Ver pedido de informação';

  @override
  String get taskSearch => 'Melhorar a pesquisa de arquivos';

  @override
  String get taskSearchNote => 'Verificando o índice do diretório';

  @override
  String get taskTest => 'Corrigir recuperação de chat';

  @override
  String get taskTestNote => 'Execução de testes';

  @override
  String get taskDone => 'Atualize o README do projeto';

  @override
  String get taskDoneNote => '3 arquivos alterados';

  @override
  String get ago => 'Apenas agora';

  @override
  String get minutesAgo => '12 minutos atrás';

  @override
  String get allow => 'Permitir uma vez';

  @override
  String get deny => 'Negar';

  @override
  String get approved => 'Permitido · Amostra grátis';

  @override
  String get denied => 'Negado · Amostra';

  @override
  String get answered => 'Respondeu · Amostra';

  @override
  String get awaiting => 'A aguardar aprovação';

  @override
  String get working => 'Em andamento';

  @override
  String get viewChanges => 'Ver as alterações';

  @override
  String get viewConversation => 'Ver o chat';

  @override
  String get chatTitle => 'Atualizar o layout de login';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => 'Hoje às 09:36';

  @override
  String get userMessage =>
      'Ajuste o espaçamento de login e unifique os estilos de entrada e botão, preservando a lógica de login';

  @override
  String get assistantMessage =>
      'Eu verifiquei a página de login e componentes de formulário compartilhados, espaçamento de entrada unificado e estilos de foco de teclado adicionados';

  @override
  String get replyPreview => 'Exemplo de fluxo de tarefas';

  @override
  String get phaseThinking => 'Pensamento';

  @override
  String get phaseReading => 'Leitura de arquivos';

  @override
  String get phaseQuestion => 'Aguardando uma resposta';

  @override
  String get phaseEditing => 'Edição de arquivos';

  @override
  String get phaseApproval => 'A aguardar aprovação';

  @override
  String get phaseTesting => 'Execução de testes';

  @override
  String get phaseReply => 'Respondendo';

  @override
  String get phaseFollowup => 'Fila de processamento';

  @override
  String get phaseComplete => 'Concluído';

  @override
  String get phaseFailed => 'Os testes falharam';

  @override
  String get allowShort => 'Permitir';

  @override
  String get queueShort => 'Fila';

  @override
  String get confirmShort => 'Confirmar';

  @override
  String get todoShort => 'O que fazer';

  @override
  String get todoInspect => 'Inspecione a página de login';

  @override
  String get todoEdit => 'Ajustar estilos de formulário';

  @override
  String get todoTest => 'Executar testes de projeto';

  @override
  String get todoNarrow => 'Verifique o espaçamento estreito da tela';

  @override
  String get workProcess => 'Atividade';

  @override
  String workSteps(String count) {
    return '· $count passos';
  }

  @override
  String get questionRecord => 'Confirmar o layout';

  @override
  String get answerRecorded => 'Respondeu';

  @override
  String get playFlow => 'Jogar a tarefa';

  @override
  String get pauseFlow => 'Pausar a demo';

  @override
  String get nextFlow => 'O próximo passo';

  @override
  String get replyingNow => 'Respondendo';

  @override
  String get toolReadLabel => 'Ler';

  @override
  String get toolEditLabel => 'Editar';

  @override
  String get toolRunLabel => 'Executar';

  @override
  String get readGroup => '3 arquivos';

  @override
  String get readFileResult => 'Arquivo lido';

  @override
  String get readFileProgress => 'Lendo o arquivo';

  @override
  String get flowAttachment =>
      'Atualização de login: unifique o espaçamento do formulário, adicione estilos de foco do teclado e preserve a lógica de login';

  @override
  String get readResult =>
      'Ler Login.tsx e estilos de formulário compartilhados\nA largura do botão móvel difere do formulário';

  @override
  String get layoutFindings =>
      'O formulário de login usa o espaçamento da área de trabalho e o botão móvel não preenche seu contêiner';

  @override
  String get layoutQuestion =>
      'O botão de login móvel deve preencher a largura?';

  @override
  String get questionPending => 'Aguardando sua resposta';

  @override
  String get wideButton => 'Usar botão de largura total';

  @override
  String get keepButton => 'Manter a largura atual';

  @override
  String get editPlan =>
      'Vou preservar a lógica de login, unificar o espaçamento e tornar o botão móvel de largura total';

  @override
  String get editPlanKeep =>
      'Vou manter a largura do botão e a lógica de login, ajustando apenas os estilos de espaçamento e foco';

  @override
  String get editThinking =>
      'Reutilize variáveis de estilo existentes e limite as alterações de layout ao formulário de login';

  @override
  String get editResult =>
      'Atualizado 3 arquivos\nEstilos de foco adicionados e regras de layout móvel';

  @override
  String get beforeTest =>
      'As alterações de layout estão concluídas. Em seguida, executarei testes de projeto para verificar regressões';

  @override
  String get testTool => 'Executar testes de projeto';

  @override
  String get testProgress =>
      'Executando testes de formulário de login…\nVerificando foco e interação do teclado';

  @override
  String get testResult =>
      '12 testes passados\nNenhuma regressão de lógica de login encontrada';

  @override
  String get testFailure =>
      'Teste de ordem de foco falhou\nO foco esperado no campo de senha, mas permaneceu no campo de nome de usuário';

  @override
  String get testFailed => 'Os testes falharam';

  @override
  String get flowResult =>
      'Os estilos de espaçamento e foco de login são unificados, com um botão móvel de largura total.Todos os 12 testes foram aprovados e a lógica de login permanece inalterada';

  @override
  String get queueSample =>
      'Verifique o espaçamento dos botões em telas estreitas também';

  @override
  String queueCount(String count) {
    return '$count mensagens em fila';
  }

  @override
  String queuePaused(String count) {
    return 'Fila em pausa · $count';
  }

  @override
  String get pauseQueue => 'Pause queue';

  @override
  String get resumeQueue => 'Retomar a fila';

  @override
  String get sendNext => 'Enviar próximo';

  @override
  String get enqueue => 'Adicionar à lista';

  @override
  String get queuedPreview => 'Adicionado à fila de amostras';

  @override
  String get moveUp => 'Mover para cima';

  @override
  String get followupThinking =>
      'Verifique os pontos de interrupção existentes para confirmar o espaçamento consistente dos botões em telas estreitas';

  @override
  String get followupTool => 'Verificar estilos de tela estreita';

  @override
  String get followupToolResult =>
      '320px e 390px usam as mesmas regras de espaçamento';

  @override
  String get followupResult =>
      'O espaçamento entre botões de tela estreita é consistente; nenhuma alteração adicional é necessária';

  @override
  String get deniedResult =>
      'Os testes não foram executados; as alterações atuais são preservadas';

  @override
  String get thinkingNow => 'Pensamento';

  @override
  String get toolsNow => 'Executando';

  @override
  String get toolPending => 'Não iniciado';

  @override
  String get thoughtLive =>
      'Primeiro, inspecione a página de login e os componentes do formulário para identificar alterações de espaçamento e foco';

  @override
  String get toolsShort => '3 ações';

  @override
  String get thought => 'Raciocínio';

  @override
  String get thoughtContent =>
      'Reutilize componentes de formulário existentes e ajuste apenas o layout de login e os estilos de foco';

  @override
  String get toolsComplete => '3 ações concluídas';

  @override
  String get toolRead => 'Ler componentes de login e formulário';

  @override
  String get toolEdit => 'Atualizar estilos de espaçamento e foco';

  @override
  String get toolDiff => 'Inspecionar diffs de arquivos';

  @override
  String get changedFiles => '3 arquivos alterados';

  @override
  String get approvalBody =>
      'Execute testes na árvore de trabalho sailry-web no Studio';

  @override
  String get approvalResolved => 'Aprovação resolvida';

  @override
  String get chatContinue => 'Continue descrevendo sua tarefa';

  @override
  String get describeTask => 'Descreva sua tarefa';

  @override
  String get send => 'Enviar';

  @override
  String get attach => 'Anexar';

  @override
  String get voice => 'Entrada de voz';

  @override
  String get voiceNote => 'Esta visualização não acessa o microfone';

  @override
  String get attachmentNote => 'Sample attachment added';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => 'Remover o anexo';

  @override
  String get sentPreview => 'Apenas visualização, não enviada';

  @override
  String get model => 'Modelo';

  @override
  String get modelSource => 'Configuração atual do chat · Studio';

  @override
  String get copy => 'Copiar';

  @override
  String get copied => 'Copiado';

  @override
  String get copyFailed => 'Cópia indisponível; selecione o texto manualmente';

  @override
  String get more => 'Mais';

  @override
  String get close => 'Fechar';

  @override
  String get back => 'Voltar';

  @override
  String get cancel => 'Cancelar';

  @override
  String get save => 'Salvar';

  @override
  String get select => 'Selecionar';

  @override
  String get sessionActions => 'Ações de chat';

  @override
  String get queue => 'Fila de mensagens';

  @override
  String get queueEmpty => 'Sem mensagens em fila';

  @override
  String get fork => 'Ramificar conversa';

  @override
  String get forked => 'Sample fork criado';

  @override
  String get archive => 'Arquivo de chat';

  @override
  String get archived => 'Arquivado em preview';

  @override
  String get stop => 'Parar a tarefa';

  @override
  String get stopped => 'Tarefa interrompida na visualização';

  @override
  String get stoppedStatus => 'Parado';

  @override
  String get hostSubtitle => 'Seus Nós de Execução';

  @override
  String get pair => 'Conectar o host';

  @override
  String get online => 'On-line';

  @override
  String get offline => 'Off-line';

  @override
  String get connection => 'Conexão';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 cores disponíveis';

  @override
  String get laptopSystem => 'Última vez online há 2 horas';

  @override
  String get statusHealthy => 'Saudável';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => 'Memória';

  @override
  String get disk => 'Disco';

  @override
  String get metrics => 'Utilização de recursos';

  @override
  String get activity => 'Atividade';

  @override
  String get lastHour => '60 minutos';

  @override
  String get sessionCount => 'Conversas';

  @override
  String get terminalCount => 'Terminais';

  @override
  String get projectCount => 'Projetos';

  @override
  String get processes => 'Processos';

  @override
  String get process => 'Nome';

  @override
  String get network => 'Rede';

  @override
  String get details => 'Detalhes';

  @override
  String get manageHost => 'Detalhes do host';

  @override
  String get hostProjects => 'Hospedar projetos';

  @override
  String get connectionDetails => 'Detalhes da conexão';

  @override
  String get direct => 'Direto';

  @override
  String get relay => 'Relé';

  @override
  String get latency => 'Latência';

  @override
  String get hostOffline =>
      'Host offline; mostrando seu último estado conhecido';

  @override
  String get retry => 'Tentar novamente';

  @override
  String get retryNote => 'A visualização não está conectada a um host real';

  @override
  String get pairTitle => 'Conectar um host';

  @override
  String get pairDescription =>
      'Digite o código de emparelhamento de 6 dígitos mostrado no host';

  @override
  String get pairCode => 'Código de emparelhamento';

  @override
  String get pairHint => 'O código de emparelhamento expira em 60 segundos';

  @override
  String get pairDemo => 'Simulação de conexão';

  @override
  String get pairSuccess => 'Host de amostra adicionado';

  @override
  String get pairInvalid => 'Digite 6 dígitos';

  @override
  String get workspace => 'Espaço de trabalho';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => 'Escolha o host';

  @override
  String get selectProject => 'Escolha o projeto';

  @override
  String get selectBranch => 'Escolha a árvore de trabalho';

  @override
  String get mainBranch => 'Árvore de trabalho principal';

  @override
  String get featureBranch => 'Layout de login';

  @override
  String get connectionTools => 'Conexões';

  @override
  String get workspaceResources => 'Espaço de trabalho';

  @override
  String get confirm => 'Confirmar';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => 'Mudanças, ramos e história';

  @override
  String get gitBranches => 'Branches';

  @override
  String get gitHistory => 'Histórico';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added linhas adicionadas · $removed removidas';
  }

  @override
  String get gitActions => 'Git ações';

  @override
  String get gitFetch => 'Obter';

  @override
  String get gitPull => 'Puxar';

  @override
  String get gitPush => 'Empurrar';

  @override
  String get gitCurrent => 'Ramo atual';

  @override
  String get gitCreateBranch => 'Novo ramo de negócios';

  @override
  String get gitBranchName => 'Nome da filial';

  @override
  String get gitSwitch => 'Mudar de ramo';

  @override
  String get gitMerge => 'Merge branch';

  @override
  String get gitDeleteBranch => 'Excluir ramo';

  @override
  String get gitHistoryLayout => 'Ajuste o espaçamento do formulário de login';

  @override
  String get gitHistoryInit => 'Inicializar página de login';

  @override
  String get gitPreview => 'Git apenas simulação; repositório inalterado';

  @override
  String get gitDirty => 'Fazer commit das alterações atuais primeiro';

  @override
  String get gitSwitchNote =>
      'Mudar ramo nesta árvore de trabalho; apenas simulação';

  @override
  String get gitDeleteNote => 'Excluir ramo selecionado; apenas simulação';

  @override
  String get gitInvalidBranch => 'O nome ou ramo inválido já existe';

  @override
  String get review => 'Revisar';

  @override
  String get browseFiles => 'Navegar na worktree';

  @override
  String get reviewFiles => 'Ver alterações de código';

  @override
  String get selectWorkspace => 'Projeto e árvore de trabalho';

  @override
  String get resourceSummary => '2 conversas · 1 terminal';

  @override
  String get searchFiles => 'Pesquisar arquivos';

  @override
  String get recentFiles => 'Arquivos';

  @override
  String get src => 'Fonte';

  @override
  String get folder => 'Pasta';

  @override
  String get modified => 'Modificado';

  @override
  String get filePreview => 'Prévia do arquivo';

  @override
  String get fileSample => 'Conteúdo do arquivo de amostra';

  @override
  String get edit => 'Editar';

  @override
  String get savePreview => 'Alterações salvas nesta visualização';

  @override
  String get unsaved => 'Não salvo';

  @override
  String get discard => 'Descartar as alterações';

  @override
  String get discardConfirm => 'Descartar alterações não salvas neste arquivo?';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => 'Ver mais recursos';

  @override
  String get ssh => 'SSH';

  @override
  String get database => 'Base de dados';

  @override
  String get ports => 'Encaminhamento de porta';

  @override
  String get browser => 'Pré-visualização da Web';

  @override
  String get portsSub => '1 avançado';

  @override
  String get connectionOwner => 'Execution Node · Estúdio de luxo';

  @override
  String get openTerminal => 'Abra o terminal';

  @override
  String get tables => 'Tabelas';

  @override
  String get portNote => 'Sample forward · Nenhum ouvinte de porta local';

  @override
  String get portTarget => 'Porto de destino';

  @override
  String get localPort => 'O porto local';

  @override
  String get closePort => 'Fechar para frente';

  @override
  String get portClosed => 'Amostra para a frente fechada';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => 'Árvore de trabalho';

  @override
  String get staged => 'Estágio';

  @override
  String get diffSummary => '3 arquivos';

  @override
  String get stage => 'Fase de todos';

  @override
  String get unstage => 'Desfazer';

  @override
  String get commit => 'Commit';

  @override
  String get commitTitle => 'Fazer commit de alterações';

  @override
  String get commitMessage => 'Enviar mensagem';

  @override
  String get commitPlaceholder => 'Descreva as mudanças';

  @override
  String get commitPreview => 'Simular commit';

  @override
  String get committed => 'Sample commit concluído';

  @override
  String get noChanges => 'Nenhumas alterações para confirmar';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => 'Continue a editar';

  @override
  String get diffSelection => 'Escolha o arquivo alterado';

  @override
  String get terminalKeyboard => 'Teclado';

  @override
  String get terminalEnter => 'Enter';

  @override
  String get terminalOutputLabel => 'Saída do terminal';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => 'Somente leitura';

  @override
  String get takeControl => 'Assuma o controle';

  @override
  String get hasControl => 'Controle de entrada';

  @override
  String get releaseControl => 'Controle de liberação';

  @override
  String get terminalPlaceholder => 'Digite um comando de exemplo';

  @override
  String get terminalPreview =>
      'Exemplo de terminal · Os comandos não são executados';

  @override
  String get terminalOutput =>
      'Comando recebido na visualização, não executado';

  @override
  String get terminalControlNote =>
      'Assuma o controle para enviar entrada; simulado aqui';

  @override
  String get usageSubtitle => 'Sailry chats somente';

  @override
  String get week => 'Esta semana';

  @override
  String get month => 'Este mês';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => 'Responses';

  @override
  String usageCoverage(String priced, String total) {
    return '$priced / $total respostas com preço';
  }

  @override
  String get usageEmpty => 'Sem dados de uso';

  @override
  String get estimatedCost => 'Custo estimado';

  @override
  String get costCoverage => '42 / 48 respostas com preço';

  @override
  String get partial => 'Dados parciais';

  @override
  String get sourcesPartial => '2 / 3 hosts atualizados';

  @override
  String get input => 'Entrada';

  @override
  String get output => 'Saída';

  @override
  String get cached => 'Hits de cache';

  @override
  String get modelUsage => 'Distribuição do modelo';

  @override
  String get hostUsage => 'Uso do host';

  @override
  String get recentRequests => 'Respostas recentes';

  @override
  String get allUsage => 'Detalhes do uso';

  @override
  String get usageNote =>
      'Os custos são estimativas; algumas respostas não têm preço';

  @override
  String get sourceNote =>
      'Os hosts offline retêm seus últimos dados conhecidos';

  @override
  String get profileSubtitle => 'Controlador para dispositivos móveis';

  @override
  String get localSettings => 'As preferências locais';

  @override
  String get nodeSettings => 'Configurações de execução Node';

  @override
  String get appearance => 'Aparência';

  @override
  String get notifications => 'Notificações';

  @override
  String get enabled => 'Ativado';

  @override
  String get disabled => 'Desativado';

  @override
  String get add => 'Adicionar';

  @override
  String get configName => 'Nome';

  @override
  String get configEndpoint => 'Ponto final';

  @override
  String get configModels => 'Modelos';

  @override
  String get configInstructions => 'Instruções';

  @override
  String get configContent => 'Conteúdo';

  @override
  String get configEmpty => 'Não há entradas';

  @override
  String get configDuplicate => 'O nome já existe';

  @override
  String configDelete(String name) {
    return 'Excluir “$name”?';
  }

  @override
  String get speechInput => 'Entrada de voz';

  @override
  String get developerInstructions =>
      'Altere o código para a tarefa e verifique o resultado';

  @override
  String get reviewerInstructions =>
      'Revise alterações de código e identifique problemas';

  @override
  String get projectConventions => 'Convenções do projeto';

  @override
  String get memoryContent => 'Preservar o estilo de código existente';

  @override
  String get providers => 'Modelos e fornecedores';

  @override
  String get roles => 'Funções';

  @override
  String get memorySettings => 'Memória';

  @override
  String get speech => 'Discurso';

  @override
  String nodeSettingsNote(String host) {
    return 'Configuração salva em $host';
  }

  @override
  String get about => 'Sobre o Sailry';

  @override
  String get aboutBody =>
      'Pré-visualização de interação móvel, não conectada a serviços';

  @override
  String get settingsSaved => 'Configurações atualizadas nesta visualização';

  @override
  String get modelPicker => 'Escolha o modelo';

  @override
  String get nodeDefaults => 'Node padrões';

  @override
  String get providerNote => '· As credenciais permanecem na execução Node';

  @override
  String get roleNote => '· Aplica-se a novos chats';

  @override
  String get auto => 'Automático';

  @override
  String get manual => 'Pergunte cada vez';

  @override
  String get notificationsNote =>
      'Controla apenas notificações de visualização';

  @override
  String get memoryNote => 'Amostra Node memória';

  @override
  String get speechNote => 'Usa a configuração de fala de execução Node';

  @override
  String get newTaskHost => 'Anfitrião de execução';

  @override
  String get newTaskProject => 'Projeto';

  @override
  String get newTaskWorktree => 'Árvore de trabalho';

  @override
  String get create => 'Criar';

  @override
  String get taskCreated => 'Exemplo de chat criado';

  @override
  String get required => 'Descreva sua tarefa primeiro';

  @override
  String get notificationsEmpty => 'Sem novas notificações';

  @override
  String get reviewTitle => 'Referências de design';

  @override
  String get reviewIntro =>
      'As páginas seguem a fonte atual; esta visualização não estabelece a aceitação do serviço móvel';

  @override
  String get reviewConversation => 'Chats, aprovações, perguntas e fila';

  @override
  String get reviewConversationText =>
      'As tarefas mantêm a propriedade do host, projeto e árvore de trabalho; expandir registros de ferramentas e aprovações dentro de chats';

  @override
  String get reviewResources => 'Arquivos, Git, terminais e conexões';

  @override
  String get reviewResourcesText =>
      'Edição de arquivos, preparação, commits e ports mantêm seus pontos de entrada; detalhes abertos em páginas secundárias';

  @override
  String get reviewHosts => 'Conexões de host e monitoramento';

  @override
  String get reviewHostsText =>
      'Nós emparelhados, códigos de 6 dígitos, uso de recursos e processos; o estado offline não é exibido como ativo';

  @override
  String get reviewUsage => 'Uso e Node configurações';

  @override
  String get reviewUsageText =>
      'Apenas conversas do Sailry; o uso agregado preserva a indicação de completude, e os custos mostram estimativas e cobertura';

  @override
  String get reviewBoundary => 'Limite móvel';

  @override
  String get reviewBoundaryText =>
      'A ponte móvel expõe conexões, bate-papos, terminais e uso; esta visualização começa no Node, modelos, emparelhamento, terminais ou plugins';

  @override
  String get reviewVisual => 'Referências visuais';

  @override
  String get reviewVisualText =>
      'Referência 1: hierarquia de bate-papo; referência 2: cartões flexíveis e navegação flutuante; referência 3: monitoramento compacto';

  @override
  String get hostConnectPrompt => 'Conectar um host';

  @override
  String get hostDisconnected => 'Conexão perdida';

  @override
  String get language => 'Idioma';

  @override
  String get languageSystem => 'Sistema';

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
  String get backgroundConnection => 'Mantenha-se conectado em segundo plano';

  @override
  String get backgroundConnectionActive => 'Mantendo conexões de host ativas';

  @override
  String get backgroundConnectionFailed =>
      'Conexão em segundo plano não habilitada; tente novamente';

  @override
  String get resetReasoning => 'Redefinir o esforço';

  @override
  String get completionAlerts => 'Alertas de conclusão';

  @override
  String get notificationsReadAll => 'Marcar tudo como lido';

  @override
  String get notificationsOpen => 'Abrir';

  @override
  String get preferencesFailed => 'Preferências não salvas; tente novamente';

  @override
  String get connectFirst => 'Conecte um host para começar';

  @override
  String get initializing => 'Iniciando';

  @override
  String get startupFailed => 'A inicialização falhou';

  @override
  String get retryConnection => 'Tentar novamente';

  @override
  String get pairAction => 'Conectar';

  @override
  String get pairFailed => 'Conexão falhou; tente novamente';

  @override
  String get pairExpired =>
      'O código de emparelhamento expirou; obtenha um novo código';

  @override
  String get pairing => 'Conectando';

  @override
  String get hostUnavailable => 'Host não conectado';

  @override
  String get hostMetricsFailed => 'Não é possível ler o status do host';

  @override
  String get hostProcessesEmpty => 'Sem processos';

  @override
  String get hostRegisterProject => 'Adicionar um projeto';

  @override
  String get hostChooseDirectory => 'Escolha o diretório';

  @override
  String get hostChooseFile => 'Escolher arquivo';

  @override
  String get hostParentDirectory => 'Diretório pai';

  @override
  String get hostEmptyDirectory => 'O diretório está vazio';

  @override
  String get hostLoadMore => 'Carregar mais';

  @override
  String get hostProjectName => 'Nome do projeto';

  @override
  String get hostProjectPath => 'Caminho do projeto no host';

  @override
  String get hostProjectFailed => 'Não é possível adicionar projeto';

  @override
  String get hostUnknown => 'Não há dados';

  @override
  String get hostRefresh => 'Atualizar';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => 'Memória';

  @override
  String get hostMetricDisk => 'Disco';

  @override
  String get failureConflict =>
      'Conteúdo alterado; recarregar e tentar novamente';

  @override
  String get failureUnknown =>
      'Resultado não confirmado; verifique o estado do host primeiro';

  @override
  String get failureDenied => 'Permissão negada';

  @override
  String get failureUnavailable => 'Host não conectado';

  @override
  String get failureBusy => 'Tente novamente mais tarde';

  @override
  String get failureGeneric => 'A operação falhou';

  @override
  String get settingsSpeechLanguage => 'Idioma';

  @override
  String get settingsSpeechAuto => 'Detectar automaticamente';

  @override
  String get settingsSpeechChinese => 'Chinês';

  @override
  String get settingsSpeechEnglish => 'English';

  @override
  String get settingsSpeechReady => 'Modelo de discurso pronto';

  @override
  String get settingsSpeechDownload => 'Baixar modelo de discurso';

  @override
  String get settingsSpeechFailed =>
      'Modelo de fala não está pronto; tente novamente';

  @override
  String get settingsNoHost => 'Conecte um host primeiro';

  @override
  String get settingsUnavailable => 'Não disponível';

  @override
  String get settingsLoadFailed => 'Não foi possível carregar';

  @override
  String get settingsSaveFailed => 'Salvar falhou; rascunho mantido';

  @override
  String get settingsConflict =>
      'Configurações alteradas; reabra e tente novamente';

  @override
  String get settingsUnknown =>
      'Resultado não confirmado; atualize para verificar';

  @override
  String get settingsRetry => 'Tentar novamente';

  @override
  String get settingsLoading => 'Carregando';

  @override
  String get settingsRequired => 'Digite um valor';

  @override
  String get settingsKey => 'ID';

  @override
  String get settingsDescription => 'Descrição';

  @override
  String get settingsInstructions => 'Instruções';

  @override
  String get settingsModels => 'IDs de modelo, um por linha';

  @override
  String get settingsApi => 'Formato API';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'Chave de API';

  @override
  String get settingsEnabled => 'Ativado';

  @override
  String get settingsDriver => 'Serviço';

  @override
  String get settingsModel => 'Modelo';

  @override
  String get settingsMemoryAuto => 'Gravar automaticamente';

  @override
  String get settingsMemoryBudget => 'Bytes de contexto';

  @override
  String get settingsMemoryReview => 'Intervalo de revisão (dias)';

  @override
  String get settingsMemoryRecords => 'Entradas de memória';

  @override
  String get settingsMemoryKind => 'Tipo';

  @override
  String get settingsMemoryUser => 'Usuário';

  @override
  String get settingsMemoryFeedback => 'Feedback';

  @override
  String get settingsMemoryProject => 'Projeto';

  @override
  String get settingsMemoryReference => 'Referência';

  @override
  String get settingsArchived => 'Arquivado';

  @override
  String get settingsEmpty => 'Não há registros';

  @override
  String get settingsUsageUnknown => 'Desconhecido';

  @override
  String get settingsUsagePartial => 'Alguns hosts estão indisponíveis';

  @override
  String get settingsUsageCache => 'Em cache';

  @override
  String get settingsUsageInput => 'Entrada sem cache';

  @override
  String get settingsUsageOutput => 'Saída';

  @override
  String get settingsUsageDaily => 'Diariamente';

  @override
  String get settingsUsageWeekly => 'Semanal';

  @override
  String get settingsUtc => 'UTC';
}

/// The translations for Portuguese, as used in Brazil (`pt_BR`).
class AppLocalizationsPtBr extends AppLocalizationsPt {
  AppLocalizationsPtBr() : super('pt_BR');
}
