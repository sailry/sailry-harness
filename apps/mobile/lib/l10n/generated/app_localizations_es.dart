// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Spanish Castilian (`es`).
class AppLocalizationsEs extends AppLocalizations {
  AppLocalizationsEs([String locale = 'es']) : super(locale);

  @override
  String get updatesVersion => 'Versión';

  @override
  String get updatesCheck => 'Buscar actualizaciones';

  @override
  String get updatesChecking => 'Comprobando';

  @override
  String updatesAvailable(String version) {
    return 'La versión $version está disponible';
  }

  @override
  String get updatesDownload => 'Descargar la actualización';

  @override
  String get updatesCurrent => 'Estás al día';

  @override
  String get updatesUnpublished => 'No hay lanzamiento móvil todavía';

  @override
  String get updatesCheckFailed => 'No se pudo buscar actualizaciones';

  @override
  String get updatesOpenFailed => 'No se pudo abrir la descarga';

  @override
  String get retryTask => 'Reintentar';

  @override
  String get welcomeTitle => '¿En qué te gustaría trabajar hoy?';

  @override
  String get welcomeExplore => 'Explorar un proyecto';

  @override
  String get welcomeExploreDetail =>
      'Comprender su estructura y puntos de entrada';

  @override
  String get welcomeExplorePrompt =>
      'Ayúdame a entender este proyecto, incluyendo sus módulos clave y puntos de entrada.';

  @override
  String get welcomeBuild => 'Construye una idea';

  @override
  String get welcomeBuildDetail => 'Dale vida a tu idea';

  @override
  String get welcomeBuildPrompt =>
      'Quiero agregar una característica a este proyecto, primero confirme los requisitos conmigo y esboce un plan de implementación.';

  @override
  String get welcomeReview => 'Revisar los cambios';

  @override
  String get welcomeReviewDetail => 'Comprobar cambios y posibles problemas';

  @override
  String get welcomeReviewPrompt =>
      'Revise los cambios actuales en este proyecto, centrándose en posibles problemas y pruebas faltantes.';

  @override
  String get welcomePlan => 'Hacer un plan';

  @override
  String get welcomePlanDetail => 'Aclarar metas y pasos';

  @override
  String get welcomePlanPrompt =>
      'Ayúdame a crear un plan paso a paso para el próximo trabajo de desarrollo.';

  @override
  String get conversationEmpty => 'Describe tu tarea';

  @override
  String get conversationLoading => 'Cargando el chat';

  @override
  String get conversationReconnecting => 'Reconectando';

  @override
  String get conversationErrorDetails => 'Ver razón';

  @override
  String conversationModelRetrying(String attempt, String limit) {
    return 'Reintentando solicitud de modelo $attempt/$limit';
  }

  @override
  String conversationModelRetried(String attempt, String limit) {
    return 'Solicitud de modelo reintentada $attempt/$limit';
  }

  @override
  String conversationToolsCount(String count) {
    return '$count llamadas a herramientas';
  }

  @override
  String get conversationGoal => 'Objetivo';

  @override
  String get conversationGoalBlocked => 'Bloqueado';

  @override
  String conversationGoalBudget(String count) {
    return 'Presupuesto $count tokens';
  }

  @override
  String get conversationStepSkipped => 'Omitido';

  @override
  String get conversationChild => 'Subtarea';

  @override
  String get conversationChildReadonly => 'Chat de subtareas';

  @override
  String get conversationOffline => 'Se perdió la conexión';

  @override
  String get conversationUnavailable => 'Chat no disponible';

  @override
  String get conversationFailed => 'Operación fallida; vuelva a intentarlo';

  @override
  String get conversationUnknown =>
      'Resultado no confirmado; comprueba el chat antes de actuar de nuevo';

  @override
  String get conversationCheckResult => 'Compruebe el resultado';

  @override
  String get conversationConflict =>
      'Configuración cambiada; vuelva a abrir antes de actuar de nuevo';

  @override
  String get conversationOlder => 'Cargar mensajes más antiguos';

  @override
  String get conversationNew => 'Un nuevo chat';

  @override
  String get conversationNoHost => 'Conectar un host primero';

  @override
  String get conversationNoProject => 'Agregar un proyecto en el host primero';

  @override
  String get conversationNoModel => 'Configure primero un modelo en el host';

  @override
  String get conversationNoTasks => 'No hay chats todavía';

  @override
  String get conversationNoMessages => 'No hay mensajes';

  @override
  String get conversationPreviewUnavailable => 'Mensaje no disponible';

  @override
  String get conversationInterrupted => 'Interrumpido';

  @override
  String get conversationFailedStatus => 'Fallido';

  @override
  String get conversationStopping => 'Deteniendo';

  @override
  String get conversationQueued => 'En cola';

  @override
  String get conversationProcessing => 'Procesamiento';

  @override
  String get conversationUnsynced => 'Estado no sincronizado';

  @override
  String get conversationGenerating => 'Respondiendo';

  @override
  String get conversationWaiting => 'A la espera de confirmación';

  @override
  String get conversationCompacting => 'Compactación del contexto';

  @override
  String get conversationForkConfirm =>
      '¿Quieres crear un chat a partir de este registro?';

  @override
  String get conversationCompacted => 'Contexto compactado';

  @override
  String get conversationToolWaiting => 'Pendiente';

  @override
  String get conversationToolRunning => 'En ejecución';

  @override
  String get conversationToolReturned => 'Devuelto';

  @override
  String get conversationToolCancelled => 'Cancelado';

  @override
  String get conversationToolNotExecuted => 'No ejecutado';

  @override
  String get conversationToolInterrupted => 'Interrumpido';

  @override
  String get conversationUnsupportedInput =>
      'Manejar esta entrada en el escritorio';

  @override
  String get conversationStartCoding => 'Iniciar la ejecución';

  @override
  String get conversationPlanFeedback => 'Sugerir cambios';

  @override
  String get conversationOther => 'Otro';

  @override
  String get conversationSubmit => 'Enviar';

  @override
  String get conversationSource => 'Fuente';

  @override
  String get conversationMode => 'Modo de trabajo';

  @override
  String get conversationCode => 'Ejecutar';

  @override
  String get conversationPlan => 'Plan';

  @override
  String get conversationPermission => 'Permisos';

  @override
  String get conversationAsk => 'Pregunta cada vez';

  @override
  String get conversationProject => 'Acceso al proyecto';

  @override
  String get conversationFull => 'Acceso completo';

  @override
  String get conversationReasoning => 'Esfuerzo de razonamiento';

  @override
  String get conversationDefault => 'Predeterminado';

  @override
  String get conversationNone => 'Desactivado';

  @override
  String get conversationMinimal => 'Mínimo';

  @override
  String get conversationLow => 'Bajo';

  @override
  String get conversationMedium => 'Medio';

  @override
  String get conversationHigh => 'Alto';

  @override
  String get conversationXHigh => 'Superior';

  @override
  String get conversationMax => 'Máximo';

  @override
  String get conversationBudget => 'Razonamiento del presupuesto';

  @override
  String get conversationAttachment => 'Adjunto';

  @override
  String get conversationAttachmentTooLarge =>
      'Archivo adjunto ilegible o mayor de 64 MB';

  @override
  String get conversationDownload => 'Ver archivo adjunto';

  @override
  String get conversationImageFailed => 'No se puede mostrar la imagen';

  @override
  String get conversationDownloadFailed =>
      'No se pudo cargar el archivo adjunto';

  @override
  String get conversationReadonly => 'Este chat está archivado';

  @override
  String get conversationMicrophoneDenied => 'No se puede acceder al micrófono';

  @override
  String get conversationRecordingFailed => 'Falló el reconocimiento de voz';

  @override
  String get conversationSpeechDisabled => 'La entrada de voz está desactivada';

  @override
  String get conversationSpeechMissing =>
      'Descarga primero el modelo de voz en Configuración';

  @override
  String get conversationRecording => 'Grabación';

  @override
  String get conversationTranscribing => 'Transcripción';

  @override
  String get conversationRecordReady => 'Listo para grabar';

  @override
  String get conversationStartRecording => 'Iniciar la grabación';

  @override
  String get conversationFinishRecording => 'Finalizar la grabación';

  @override
  String get conversationSources => 'Fuentes';

  @override
  String get conversationSearchSuggestions => 'Sugerencias de búsqueda';

  @override
  String get conversationStats => 'Uso del chat';

  @override
  String get conversationStatsEmpty => 'No hay uso todavía';

  @override
  String get conversationStatsOverview => 'Panorama general';

  @override
  String get conversationStatsTokenGroup => 'Uso de tokens';

  @override
  String get conversationStatsCostGroup => 'Costo';

  @override
  String get conversationStatsGenerationGroup => 'Generación';

  @override
  String get conversationStatsTokens => 'Tokens';

  @override
  String get conversationStatsInput => 'Entrada';

  @override
  String get conversationStatsOutput => 'Salida';

  @override
  String get conversationStatsCached => 'Entrada en caché';

  @override
  String get conversationStatsReasoning => 'Salida de razonamiento';

  @override
  String get conversationStatsCacheRate => 'Hits de caché';

  @override
  String get conversationStatsCost => 'Costo estimado';

  @override
  String get conversationStatsCostCoverage => 'Cobertura de costos';

  @override
  String get conversationStatsSpeed => 'Velocidad de generación';

  @override
  String conversationStatsSpeedValue(String value) {
    return '$value tok/s';
  }

  @override
  String get conversationStatsTimingCoverage => 'Cobertura de tiempo';

  @override
  String get conversationStatsTurns => 'Turnos';

  @override
  String get conversationStatsResponses => 'Modelo de respuestas';

  @override
  String get conversationStatsContext => 'Contexto actual';

  @override
  String get conversationStatsInputCost => 'Costo de los insumos';

  @override
  String get conversationStatsOutputCost => 'Costo de la producción';

  @override
  String get conversationStatsCacheReadCost => 'Costo de lectura de caché';

  @override
  String get conversationStatsCacheWriteCost => 'Costo de escritura en caché';

  @override
  String get messageHistoryUpdated =>
      'Chat actualizado; registros originales conservados';

  @override
  String get turnUndoUnsaved =>
      'Los archivos tienen cambios no guardados; guárdelos o deséchelos primero';

  @override
  String get codePlain => 'Texto sin formato';

  @override
  String get toolArguments => 'Argumentos';

  @override
  String get toolResult => 'Resultado';

  @override
  String get toolRaw => 'Resultado en bruto';

  @override
  String get turnChanges => 'Cambios de giro';

  @override
  String turnChangesCount(String count) {
    return '$count archivos';
  }

  @override
  String get turnUndo => 'Deshacer los cambios';

  @override
  String get turnUndoAll => 'Deshacer todo';

  @override
  String get turnUndoConfirm =>
      'Los conflictos con los cambios posteriores detendrán la operación, y el archivo se devolverá a la ubicación original';

  @override
  String get turnUndoDone => 'Deshecho';

  @override
  String get turnUndoPartial =>
      'Deshacer algunos cambios; comprobar los archivos restantes';

  @override
  String get messageActions => 'Acciones de mensaje';

  @override
  String get messageEdit => 'Editar y regenerar';

  @override
  String get messageEditConfirm =>
      '¿Reemplazar este mensaje y el siguiente chat? Los archivos no se revertirán';

  @override
  String get messageRewind => 'Rebobina aquí';

  @override
  String get messageRewindConfirm =>
      'Los registros de chat posteriores se guardarán en una copia de seguridad; los archivos no se revertirán';

  @override
  String get messageBackup => 'Ver copia de seguridad de chat';

  @override
  String get messageRegenerate => 'Regenerar';

  @override
  String get messageSearch => 'Buscar en chat';

  @override
  String get messageSearchHint => 'Buscar mensajes';

  @override
  String get messageSearchMissing =>
      'Este mensaje ya no está en el chat actual';

  @override
  String get messageSearchStale => 'Chat cambiado; buscar de nuevo';

  @override
  String get messageNoResults => 'No hay mensajes coincidentes';

  @override
  String get messageCheck => 'Compruebe el resultado de la operación';

  @override
  String get messageReference => 'Referencia';

  @override
  String get messageReferenceContext =>
      'Esta referencia pertenece al contexto en el que se envió el mensaje';

  @override
  String get toolFailed => 'Fallido';

  @override
  String toolExitCode(String code) {
    return 'Código de salida $code';
  }

  @override
  String toolSignal(String signal) {
    return 'Terminado por la señal $signal';
  }

  @override
  String get toolTimedOut => 'El tiempo de espera del comando se agotó';

  @override
  String get toolCancelled => 'Comando cancelado';

  @override
  String get toolOutcomeUnknown => 'Resultado del comando desconocido';

  @override
  String get toolQuestionAnswered => 'Respondido';

  @override
  String get toolQuestionDeclined => 'Rechazada';

  @override
  String get toolQuestionCancelled => 'Cancelado';

  @override
  String get fileLinkUnavailable => 'No se puede abrir este enlace';

  @override
  String get imagePreview => 'Vista previa de imagen';

  @override
  String get fileOpenExternal => 'Abrir con otra aplicación';

  @override
  String get fileOpenFailed => 'No se puede abrir el archivo';

  @override
  String get fileNoApplication => 'Ninguna aplicación puede abrir este archivo';

  @override
  String get fileSaveBeforeShare => '¿Guardar los cambios antes de compartir?';

  @override
  String fileTrashConfirm(String name) {
    return '¿Mover “$name” a la papelera del host? Los cambios no guardados también se descartarán';
  }

  @override
  String get fileTrashUncertain =>
      'Resultado de eliminación no confirmado; vuelva a intentar la consulta';

  @override
  String get fileSaveFailed => 'El archivo no se pudo guardar';

  @override
  String get terminalHideKeyboard => 'Ocultar el teclado';

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
  String get terminalArrowLeft => 'Izquierda';

  @override
  String get terminalArrowUp => 'Arriba';

  @override
  String get terminalArrowDown => 'Abajo';

  @override
  String get terminalArrowRight => 'Derecha';

  @override
  String get resourceNoWorkspace =>
      'Elegir un árbol de trabajo en un host conectado';

  @override
  String get resourceDisconnected => 'Host no conectado';

  @override
  String get resourceRoot => 'Raíz';

  @override
  String get resourceMore => '» Cargar más';

  @override
  String get resourcePartial => 'Contenido parcial mostrado';

  @override
  String get resourceEmpty => 'No hay contenido';

  @override
  String get resourceSaveError => 'Fallo al guardar; borrador conservado';

  @override
  String get resourceReloadConfirm =>
      '¿Descartar el borrador y cargar el contenido más reciente?';

  @override
  String get resourceWorktreeCreate => 'Nuevo árbol de trabajo';

  @override
  String get resourceSessionServices => 'Servicios de chat';

  @override
  String get resourceServicesUnavailable => 'Lista de servicios no disponible';

  @override
  String get resourceNoServices =>
      'No se han encontrado direcciones de servicio';

  @override
  String get resourceServiceOpen => 'Servicio abierto';

  @override
  String get resourceRemotePort => 'Puerto remoto';

  @override
  String get resourceOpenPort => 'Puerto delantero';

  @override
  String get resourceOpenBrowser => 'Vista previa de la página web';

  @override
  String get resourcePreviewFailed => 'No se pudo cargar la página';

  @override
  String get resourcePreviewLink =>
      'No se puede abrir este enlace en vista previa';

  @override
  String get resourceForwardStopped => 'El reenvío se detuvo';

  @override
  String get resourceTerminalControl => 'Tome el control';

  @override
  String get resourceTerminalControlHint => 'Controlado por otro dispositivo';

  @override
  String get resourceTerminalClaiming => 'Tomando el control';

  @override
  String get resourceTerminalReadOnly => 'Terminal de solo lectura';

  @override
  String get resourceTerminalEnded => 'Terminación del terminal';

  @override
  String get resourceTerminalConnecting => 'Terminal de conexión';

  @override
  String get resourceTerminalInput => 'Entrada de terminal';

  @override
  String get resourceTerminalPaste => 'Pegar';

  @override
  String get resourceGitNotRepository =>
      'Este directorio no es un repositorio Git';

  @override
  String get resourceInvalidPort => 'Ingrese un puerto de 1–65535';

  @override
  String get tool_navigate => 'Abrir la página';

  @override
  String get tool_back => 'Volver al inicio';

  @override
  String get tool_forward => 'Ir hacia adelante';

  @override
  String get tool_refresh => 'Actualizar la página';

  @override
  String get tool_right_click => 'Haga clic en el elemento';

  @override
  String get tool_clear => 'Introduzca el texto';

  @override
  String get tool_select => 'Selecciona una opción';

  @override
  String get tool_hover => 'Elemento de hover';

  @override
  String get tool_scroll => 'Desplazar la página';

  @override
  String get tool_press_key => 'Pulse la tecla';

  @override
  String get tool_new_tab => 'Una nueva pestaña';

  @override
  String get tool_list_windows => 'Pestañas del navegador';

  @override
  String get tool_switch_window => 'Pestaña de interruptor';

  @override
  String get tool_close_window => 'Cerrar la ventana';

  @override
  String get tool_close_session => 'Cerrar el navegador';

  @override
  String get tool_screenshot => 'Página de captura';

  @override
  String get tool_print_to_pdf => 'Exportar a PDF';

  @override
  String get tool_file_upload => 'Subir el archivo';

  @override
  String get tool_downloads => 'Ver las descargas';

  @override
  String get tool_save_download => 'Guardar el archivo descargado';

  @override
  String get tool_evaluate_js => 'Ejecutar script de página';

  @override
  String get tool_get_cookies => 'Leer las cookies';

  @override
  String get tool_delete_all_cookies => 'Cambiar las cookies';

  @override
  String get tool_drag_and_drop => 'Elemento de arrastre';

  @override
  String get tool_focus => 'Elemento de enfoque';

  @override
  String get tool_handle_alert => 'Alerta de página de manejo';

  @override
  String get tool_database_catalog => 'Explorar la base de datos';

  @override
  String get tool_database_query => 'Base de datos de consultas';

  @override
  String get tool_database_execute => 'Ejecutar operación de base de datos';

  @override
  String get tool_search_memory => 'Memoria de búsqueda';

  @override
  String get tool_review_memories => 'Revise los recuerdos';

  @override
  String get tool_consolidate_memories => 'Fusionar los recuerdos';

  @override
  String get tool_save_memory => 'Ahorra memoria';

  @override
  String get tool_forget_memory => 'Eliminar memoria';

  @override
  String get tool_update_plan => 'Plan de actualización';

  @override
  String get tool_create_goal => 'Crear un objetivo';

  @override
  String get tool_get_goal => 'Ver objetivo';

  @override
  String get tool_update_goal => 'Objetivo de actualización';

  @override
  String get tool_spawn_agent => 'Subagente';

  @override
  String get tool_browser_tabs => 'Pestañas del navegador';

  @override
  String get tool_browser_read => 'Leer la página';

  @override
  String get tool_browser_navigate => 'Abrir la página';

  @override
  String get tool_browser_click => 'Haga clic en el elemento';

  @override
  String get tool_browser_input => 'Introduzca el texto';

  @override
  String get tool_browser_scroll => 'Desplazar la página';

  @override
  String get tool_browser_back => 'Volver al inicio';

  @override
  String get tool_browser_forward => 'Ir hacia adelante';

  @override
  String get tool_browser_refresh => 'Actualizar la página';

  @override
  String get tool_browser_open => 'Una nueva pestaña';

  @override
  String get tool_browser_close => 'Cerrar la pestaña';

  @override
  String get tool_browser_focus => 'Pestaña de interruptor';

  @override
  String get tool_browser_select => 'Selecciona una opción';

  @override
  String get tool_browser_hover => 'Elemento de hover';

  @override
  String get tool_browser_key => 'Pulse la tecla';

  @override
  String get tool_browser_frame => 'Marco del interruptor';

  @override
  String get tool_browser_wait => 'Esperar por página';

  @override
  String get tool_browser_screenshot => 'Página de captura';

  @override
  String get tool_ssh_run => 'Ejecute el comando SSH';

  @override
  String get tool_ssh_transfer => 'Transferir archivo SSH';

  @override
  String get tool_list_worktrees => 'Lista de worktrees';

  @override
  String get tool_create_worktree => 'Nuevo árbol de trabajo';

  @override
  String get tool_register_worktree => 'Añadir árbol de trabajo';

  @override
  String get tool_remove_worktree => 'Eliminar el árbol de trabajo';

  @override
  String get tool_google_search => 'Buscar en la web';

  @override
  String get tool_web_fetch => 'Obtener la página';

  @override
  String get tool_fetch_url => 'Obtener la página';

  @override
  String get tool_read_file => 'Leer el archivo';

  @override
  String get tool_write_file => 'Escribir archivo';

  @override
  String get tool_list_directory => 'Explorar el directorio';

  @override
  String get tool_search_files => 'Buscar archivos';

  @override
  String get tool_run_command => 'Ejecutar el comando';

  @override
  String get tool_read_command => 'Comando Ver fondo';

  @override
  String get tool_stop_command => 'Comando de parada';

  @override
  String get tool_load_skill => 'Habilidad de carga';

  @override
  String get tool_read_skill_resource => 'Recurso de habilidad de lectura';

  @override
  String get tool_computer_desktop => 'Ver el escritorio';

  @override
  String get tool_computer_observe => 'Observar la pantalla';

  @override
  String get tool_computer_input => 'Ordenador de control';

  @override
  String get tool_computer_focus => 'Aplicación de Switch';

  @override
  String get tool_computer_open => 'Abrir la aplicación';

  @override
  String get tool_git_status => 'Git status';

  @override
  String get tool_git_diff => 'Ver diferencias';

  @override
  String get tool_git_log => 'Git log';

  @override
  String get tool_inspect_image => 'Inspeccionar la imagen';

  @override
  String get tool_generate_image => 'Generar una imagen';

  @override
  String get tool_generate_video => 'Generar un vídeo';

  @override
  String get terminalUnavailable => 'El terminal no está conectado';

  @override
  String get terminalFixture =>
      '~/Projects/sailry-web\n❯ pnpm test\n\n PASS  src/tests/login.test.ts\n PASS  src/tests/session.test.ts\n PASS  src/tests/navigation.test.ts\n\nTest Suites  3 passed, 3 total\nTests        18 passed, 18 total\nTime         2.41 s\n\n✓ All tests passed\n\n❯';

  @override
  String get refresh => 'Actualizar';

  @override
  String get loading => 'Cargando';

  @override
  String get home => 'Chats';

  @override
  String get idle => 'Inactivo';

  @override
  String get allProjects => 'Todos los proyectos';

  @override
  String get allWorktrees => 'Todos los árboles de trabajo';

  @override
  String get filterProjects => 'Filtrar los proyectos';

  @override
  String get closeSearch => 'Cerrar la búsqueda';

  @override
  String onlineHostCount(String count) {
    return '$count en línea';
  }

  @override
  String get taskActions => 'Acciones de tarea';

  @override
  String get archiveShort => 'Archivo';

  @override
  String get archiveTab => 'Archivado';

  @override
  String get archivedTasks => 'Archivado';

  @override
  String get delete => 'Eliminar';

  @override
  String get deleteTask => 'Eliminar el chat';

  @override
  String get deleteWarning =>
      'Este chat no se puede reanudar después de la eliminación';

  @override
  String get busyDelete => 'Detener la tarea antes de eliminar este chat';

  @override
  String get stopBeforeDelete => 'Detener la tarea';

  @override
  String get deleted => 'Chat eliminado de la vista previa';

  @override
  String get restored => 'Restaurado a casa';

  @override
  String get restore => 'Restaurar';

  @override
  String get archiveEmpty => 'No hay chats archivados';

  @override
  String get archiveKeepsRunning =>
      'El archivado no detiene las tareas en ejecución';

  @override
  String get title => 'Sailry · Vista previa móvil';

  @override
  String get brand => 'Sailry';

  @override
  String get mobile => 'Espacio de trabajo móvil';

  @override
  String get edition => 'EXPLORACIÓN MÓVIL / 01';

  @override
  String get intro => 'Tareas, chats y espacios de trabajo remotos';

  @override
  String get preview => 'Vista previa';

  @override
  String get sample =>
      'Datos de muestra · Los cambios permanecen en esta página';

  @override
  String get mixed => 'Luz y oscuridad';

  @override
  String get dark => 'Oscuro';

  @override
  String get light => 'Claro';

  @override
  String get gallery => 'Panorama general';

  @override
  String get focus => 'Una sola pantalla';

  @override
  String get reset => 'Restablecer vista previa';

  @override
  String get page => 'Elige una página';

  @override
  String get experience => 'Abrir la página';

  @override
  String get backGallery => 'Volver a la vista general';

  @override
  String get design => 'Características y diseño';

  @override
  String get footer => 'VELA / MÓVIL';

  @override
  String get footerNote => 'Vista previa HTML local · Sin conexión de servicio';

  @override
  String get tasks => 'Tareas';

  @override
  String get chat => 'Chat';

  @override
  String get hosts => 'Hosts';

  @override
  String get resources => 'Recursos humanos';

  @override
  String get settings => 'Configuración';

  @override
  String get usage => 'Uso';

  @override
  String get changes => 'Cambios';

  @override
  String get terminal => 'Terminal';

  @override
  String get newTerminal => 'La nueva terminal';

  @override
  String get files => 'Archivos';

  @override
  String get project => 'Proyecto';

  @override
  String get worktree => 'Árbol de trabajo';

  @override
  String get subtitleTasks =>
      'Tareas entre hosts · Aprobaciones y respuestas primero';

  @override
  String get subtitleChat =>
      'Chat continuo · Expanda la actividad de la herramienta según sea necesario';

  @override
  String get subtitleHosts => 'Conexiones, recursos de host y procesos';

  @override
  String get subtitleResources => 'Host → Proyecto → Árbol de trabajo';

  @override
  String get subtitleChanges =>
      'Diferencias de archivos, preparación y confirmaciones';

  @override
  String get subtitleTerminal =>
      'Terminal remoto · Control de entrada explícito';

  @override
  String get subtitleUsage => 'Sailry chats · Agregado entre hosts';

  @override
  String get subtitleSettings =>
      'Preferencias locales y configuración de ejecución Node';

  @override
  String get allHosts => 'Todos los hosts';

  @override
  String get connectedHosts => '2 en línea';

  @override
  String get all => 'Todo';

  @override
  String get running => 'En ejecución';

  @override
  String get waiting => 'Pendiente';

  @override
  String get completed => 'Completado';

  @override
  String get taskProgress => 'Tarea actual';

  @override
  String get taskWait => 'A la espera de su decisión';

  @override
  String get taskRecent => 'Recientemente completado';

  @override
  String get search => 'Buscar';

  @override
  String get searchTasks => 'Buscar tareas y proyectos';

  @override
  String get filterTasks => 'Tareas de filtro';

  @override
  String get noResults => 'No hay tareas coincidentes';

  @override
  String get notification => 'Notificaciones';

  @override
  String get newTask => 'Nueva tarea';

  @override
  String get newConversation => 'Un nuevo chat';

  @override
  String get approveTitle => 'Actualizar el diseño de inicio de sesión';

  @override
  String get approveNote => 'Ejecutar pruebas de proyecto';

  @override
  String get approveContext => 'sailry-web · feature/sign-in';

  @override
  String get questionTitle => 'Organizar la documentación API';

  @override
  String get questionNote => 'A la espera de respuesta';

  @override
  String get question => '¿Qué idioma debe usar la documentación?';

  @override
  String get optionChinese => 'Chino';

  @override
  String get optionEnglish => 'English';

  @override
  String get reply => 'Responder';

  @override
  String get approval => 'Aprobación';

  @override
  String get viewRequest => 'Ver la solicitud';

  @override
  String get taskSearch => 'Mejorar la búsqueda de archivos';

  @override
  String get taskSearchNote => 'Comprobación del índice del directorio';

  @override
  String get taskTest => 'Corregir la recuperación de chat';

  @override
  String get taskTestNote => 'Ejecución de pruebas';

  @override
  String get taskDone => 'Actualizar el proyecto README';

  @override
  String get taskDoneNote => '3 archivos cambiados';

  @override
  String get ago => 'En este momento';

  @override
  String get minutesAgo => 'Hace 12 horas';

  @override
  String get allow => 'Permitir una vez';

  @override
  String get deny => 'Denegar';

  @override
  String get approved => 'Permitido · Muestra';

  @override
  String get denied => 'Denegado · Muestra';

  @override
  String get answered => 'Respondido · Muestra';

  @override
  String get awaiting => 'En espera de aprobación';

  @override
  String get working => 'En curso';

  @override
  String get viewChanges => 'Ver los cambios';

  @override
  String get viewConversation => 'Ver el chat';

  @override
  String get chatTitle => 'Actualizar el diseño de inicio de sesión';

  @override
  String get chatHost => 'sailry-web · Studio';

  @override
  String get branch => 'feature/sign-in';

  @override
  String get today => 'Hoy a las 09:36';

  @override
  String get userMessage =>
      'Ajuste el espaciado de inicio de sesión y unifique los estilos de entrada y de botones mientras conserva la lógica de inicio de sesión';

  @override
  String get assistantMessage =>
      'Revisé la página de inicio de sesión y los componentes de formulario compartidos, el espaciado de entrada unificado y los estilos de enfoque del teclado añadidos';

  @override
  String get replyPreview => 'Flujo de tareas de muestra';

  @override
  String get phaseThinking => 'Pensamiento';

  @override
  String get phaseReading => 'Lectura de archivos';

  @override
  String get phaseQuestion => 'A la espera de respuesta';

  @override
  String get phaseEditing => 'Edición de archivos';

  @override
  String get phaseApproval => 'En espera de aprobación';

  @override
  String get phaseTesting => 'Ejecución de pruebas';

  @override
  String get phaseReply => 'Respondiendo';

  @override
  String get phaseFollowup => 'Cola de procesamiento';

  @override
  String get phaseComplete => 'Completado';

  @override
  String get phaseFailed => 'Las pruebas fallaron';

  @override
  String get allowShort => 'Permitir';

  @override
  String get queueShort => 'Cola';

  @override
  String get confirmShort => 'Confirmar';

  @override
  String get todoShort => 'Cosas que hacer';

  @override
  String get todoInspect => 'Inspeccionar la página de inicio de sesión';

  @override
  String get todoEdit => 'Ajustar estilos de formulario';

  @override
  String get todoTest => 'Ejecutar pruebas de proyecto';

  @override
  String get todoNarrow => 'Compruebe el espaciado estrecho de la pantalla';

  @override
  String get workProcess => 'Actividad';

  @override
  String workSteps(String count) {
    return '· $count pasos';
  }

  @override
  String get questionRecord => 'Confirmar el diseño';

  @override
  String get answerRecorded => 'Respondido';

  @override
  String get playFlow => 'Reproducir la tarea';

  @override
  String get pauseFlow => 'Pausa la demo';

  @override
  String get nextFlow => 'El siguiente paso';

  @override
  String get replyingNow => 'Respondiendo';

  @override
  String get toolReadLabel => 'Leer';

  @override
  String get toolEditLabel => 'Editar';

  @override
  String get toolRunLabel => 'Ejecutar';

  @override
  String get readGroup => '3 filas';

  @override
  String get readFileResult => 'Archivo de lectura';

  @override
  String get readFileProgress => 'Leyendo el archivo';

  @override
  String get flowAttachment =>
      'Actualización de inicio de sesión: unificar el espaciado del formulario, agregar estilos de enfoque del teclado y preservar la lógica de inicio de sesión';

  @override
  String get readResult =>
      'Leer Login.tsx y estilos de formulario compartidos\nEl ancho del botón móvil difiere del formulario';

  @override
  String get layoutFindings =>
      'El formulario de inicio de sesión utiliza el espaciado de escritorio y el botón móvil no llena su contenedor';

  @override
  String get layoutQuestion =>
      '¿Debe el botón de inicio de sesión móvil llenar el ancho?';

  @override
  String get questionPending => 'Esperando su respuesta';

  @override
  String get wideButton => 'Usar botón de ancho completo';

  @override
  String get keepButton => 'Mantener el ancho actual';

  @override
  String get editPlan =>
      'Preservaré la lógica de inicio de sesión, unificaré el espaciado y haré que el botón móvil tenga todo el ancho';

  @override
  String get editPlanKeep =>
      'Mantendré el ancho del botón y la lógica de inicio de sesión, ajustando solo el espaciado y los estilos de enfoque';

  @override
  String get editThinking =>
      'Reutilizar las variables de estilo existentes y limitar los cambios de diseño al formulario de inicio de sesión';

  @override
  String get editResult =>
      'Actualizado 3 archivos\nSe han añadido estilos de enfoque y reglas de diseño móvil';

  @override
  String get beforeTest =>
      'A continuación, ejecutaré las pruebas del proyecto para comprobar si hay regresiones';

  @override
  String get testTool => 'Ejecutar pruebas de proyecto';

  @override
  String get testProgress =>
      'Ejecutando pruebas de formulario de inicio de sesión…\nComprobación del enfoque y la interacción del teclado';

  @override
  String get testResult =>
      '12 pruebas aprobadas\nNo se han encontrado regresiones de lógica de inicio de sesión';

  @override
  String get testFailure =>
      'Falló la prueba de orden de enfoque\nEl enfoque esperado en el campo de contraseña, pero se mantuvo en el campo de nombre de usuario';

  @override
  String get testFailed => 'Las pruebas fallaron';

  @override
  String get flowResult =>
      'El espaciado de inicio de sesión y los estilos de enfoque se han unificado, con un botón móvil de ancho completo';

  @override
  String get queueSample =>
      'Comprueba también el espaciado de los botones en pantallas estrechas';

  @override
  String queueCount(String count) {
    return '$count mensajes en cola';
  }

  @override
  String queuePaused(String count) {
    return 'Cola en pausa · $count';
  }

  @override
  String get pauseQueue => 'Pausa la cola';

  @override
  String get resumeQueue => 'Reanudar la cola';

  @override
  String get sendNext => 'Enviar la siguiente';

  @override
  String get enqueue => 'Añadir a la lista';

  @override
  String get queuedPreview => 'Añadido a la cola de muestras';

  @override
  String get moveUp => 'Subir';

  @override
  String get followupThinking =>
      'Compruebe los puntos de interrupción existentes para confirmar un espaciado de botones consistente en pantallas estrechas';

  @override
  String get followupTool => 'Comprobar estilos de pantalla estrecha';

  @override
  String get followupToolResult =>
      '320px y 390px usan las mismas reglas de espaciado';

  @override
  String get followupResult =>
      'El espaciado de los botones de pantalla estrecha es consistente; no se necesitan más cambios';

  @override
  String get deniedResult =>
      'No se ejecutaron pruebas; se conservan los cambios actuales';

  @override
  String get thinkingNow => 'Pensamiento';

  @override
  String get toolsNow => 'Ejecutando';

  @override
  String get toolPending => 'No iniciado';

  @override
  String get thoughtLive =>
      'Primero inspeccione la página de inicio de sesión y los componentes del formulario para identificar los cambios de espaciado y enfoque';

  @override
  String get toolsShort => '3 acciones';

  @override
  String get thought => 'Razonamiento';

  @override
  String get thoughtContent =>
      'Reutilice los componentes de formulario existentes y ajuste solo el diseño de inicio de sesión y los estilos de enfoque';

  @override
  String get toolsComplete => '3 acciones completadas';

  @override
  String get toolRead => 'Leer componentes de inicio de sesión y formulario';

  @override
  String get toolEdit => 'Actualizar estilos de espaciado y enfoque';

  @override
  String get toolDiff => 'Inspeccionar diffs de archivos';

  @override
  String get changedFiles => '3 archivos cambiados';

  @override
  String get approvalBody =>
      'Ejecutar pruebas en el árbol de trabajo sailry-web en Studio';

  @override
  String get approvalResolved => 'Aprobación resuelta';

  @override
  String get chatContinue => 'Continúa describiendo tu tarea';

  @override
  String get describeTask => 'Describe tu tarea';

  @override
  String get send => 'Enviar';

  @override
  String get attach => 'Adjuntar';

  @override
  String get voice => 'Entrada de voz';

  @override
  String get voiceNote => 'Esta vista previa no accede al micrófono';

  @override
  String get attachmentNote => 'Se agregó un archivo adjunto de muestra';

  @override
  String get attachment => 'design-notes.md';

  @override
  String get removeAttachment => 'Eliminar el archivo adjunto';

  @override
  String get sentPreview => 'Vista previa solamente, no enviada';

  @override
  String get model => 'Modelo';

  @override
  String get modelSource => 'Configuración actual del chat · Studio';

  @override
  String get copy => 'Copiar';

  @override
  String get copied => 'Copiado';

  @override
  String get copyFailed =>
      'Copia no disponible; seleccione el texto manualmente';

  @override
  String get more => 'Más';

  @override
  String get close => 'Cerrar';

  @override
  String get back => 'Atrás';

  @override
  String get cancel => 'Cancelar';

  @override
  String get save => 'Guardar';

  @override
  String get select => 'Seleccionar';

  @override
  String get sessionActions => 'Acciones de chat';

  @override
  String get queue => 'Cola de mensajes';

  @override
  String get queueEmpty => 'No hay mensajes en cola';

  @override
  String get fork => 'Bifurcar conversación';

  @override
  String get forked => 'Bifurcación de muestra creada';

  @override
  String get archive => 'Archivo de chat';

  @override
  String get archived => 'Archivado en vista previa';

  @override
  String get stop => 'Detener la tarea';

  @override
  String get stopped => 'Tarea detenida en vista previa';

  @override
  String get stoppedStatus => 'Detenido';

  @override
  String get hostSubtitle => 'Sus nodos de ejecución';

  @override
  String get pair => 'Conectar el host';

  @override
  String get online => 'En línea';

  @override
  String get offline => 'Sin conexión';

  @override
  String get connection => 'Conexión';

  @override
  String get studio => 'Studio';

  @override
  String get server => 'Build Server';

  @override
  String get laptop => 'MacBook Air';

  @override
  String get hostSystem => 'macOS · Apple Silicon';

  @override
  String get serverSystem => 'Linux · 8 color';

  @override
  String get laptopSystem => 'Última vez en línea hace 1 hora';

  @override
  String get statusHealthy => 'Saludable';

  @override
  String get cpu => 'CPU';

  @override
  String get memory => 'Memoria';

  @override
  String get disk => 'Disco';

  @override
  String get metrics => 'Utilización de recursos';

  @override
  String get activity => 'Actividad';

  @override
  String get lastHour => 'Últimos 60 minutos';

  @override
  String get sessionCount => 'Chats';

  @override
  String get terminalCount => 'Terminales';

  @override
  String get projectCount => 'Proyectos';

  @override
  String get processes => 'Procesos';

  @override
  String get process => 'Nombre';

  @override
  String get network => 'Red';

  @override
  String get details => 'Detalles';

  @override
  String get manageHost => 'Detalles del host';

  @override
  String get hostProjects => 'Proyectos de acogida';

  @override
  String get connectionDetails => 'Detalles de conexión';

  @override
  String get direct => 'Directo';

  @override
  String get relay => 'Relé';

  @override
  String get latency => 'Latencia';

  @override
  String get hostOffline =>
      'Host desconectado; mostrando su último estado conocido';

  @override
  String get retry => 'Reintentar';

  @override
  String get retryNote => 'La vista previa no está conectada a un host real';

  @override
  String get pairTitle => 'Conectar un host';

  @override
  String get pairDescription =>
      'Ingrese el código de emparejamiento de 6 dígitos que se muestra en el host';

  @override
  String get pairCode => 'Código de emparejamiento';

  @override
  String get pairHint => 'El código de emparejamiento caduca en 60 segundos';

  @override
  String get pairDemo => 'Simula la conexión';

  @override
  String get pairSuccess => 'Host de muestra añadido';

  @override
  String get pairInvalid => 'Ingrese 6 dígitos';

  @override
  String get workspace => 'Espacio de trabajo';

  @override
  String get workspaceSub => 'Studio / sailry-web';

  @override
  String get selectHost => 'Elige un host';

  @override
  String get selectProject => 'Elige un proyecto';

  @override
  String get selectBranch => 'Elija el árbol de trabajo';

  @override
  String get mainBranch => 'Árbol de trabajo principal';

  @override
  String get featureBranch => 'Diseño de inicio de sesión';

  @override
  String get connectionTools => 'Conexiones';

  @override
  String get workspaceResources => 'Espacio de trabajo';

  @override
  String get confirm => 'Confirmar';

  @override
  String get git => 'Git';

  @override
  String get gitSummary => 'Cambios, ramas e historia';

  @override
  String get gitBranches => 'Ramas';

  @override
  String get gitHistory => 'Historial';

  @override
  String gitLineChanges(String added, String removed) {
    return '$added líneas añadidas · $removed eliminadas';
  }

  @override
  String get gitActions => 'Git acciones';

  @override
  String get gitFetch => 'Obtener';

  @override
  String get gitPull => 'Pull';

  @override
  String get gitPush => 'Push';

  @override
  String get gitCurrent => 'Sucursal actual';

  @override
  String get gitCreateBranch => 'Nueva sucursal';

  @override
  String get gitBranchName => 'Nombre de la sucursal';

  @override
  String get gitSwitch => 'Cambiar de rama';

  @override
  String get gitMerge => 'Ramificación de fusión';

  @override
  String get gitDeleteBranch => 'Eliminar rama';

  @override
  String get gitHistoryLayout =>
      'Ajustar el espaciado del formulario de inicio de sesión';

  @override
  String get gitHistoryInit => 'Inicializar página de inicio de sesión';

  @override
  String get gitPreview => 'Git simulación solamente; repositorio sin cambios';

  @override
  String get gitDirty => 'Confirmar primero los cambios actuales';

  @override
  String get gitSwitchNote =>
      'Cambiar rama en este árbol de trabajo; solo simulación';

  @override
  String get gitDeleteNote => 'Eliminar rama seleccionada; solo simulación';

  @override
  String get gitInvalidBranch => 'Nombre o rama no válida ya existe';

  @override
  String get review => 'Revisar';

  @override
  String get browseFiles => 'Navegar por worktree';

  @override
  String get reviewFiles => 'Ver cambios de código';

  @override
  String get selectWorkspace => 'Proyecto y árbol de trabajo';

  @override
  String get resourceSummary => '2 conversaciones · 1 terminal';

  @override
  String get searchFiles => 'Buscar archivos';

  @override
  String get recentFiles => 'Archivos';

  @override
  String get src => 'Fuente';

  @override
  String get folder => 'Carpeta';

  @override
  String get modified => 'Modificado';

  @override
  String get filePreview => 'Vista previa del archivo';

  @override
  String get fileSample => 'Contenido de archivo de muestra';

  @override
  String get edit => 'Editar';

  @override
  String get savePreview => 'Cambios guardados en esta vista previa';

  @override
  String get unsaved => 'Sin guardar';

  @override
  String get discard => 'Descartar los cambios';

  @override
  String get discardConfirm =>
      '¿Descartar los cambios no guardados en este archivo?';

  @override
  String get markdownExample =>
      '# Sailry Web\n\nA workspace for your next idea.\n\n## Development\n\npnpm install\npnpm dev';

  @override
  String get connections => 'Ver más recursos';

  @override
  String get ssh => 'SSH';

  @override
  String get database => 'Base de datos';

  @override
  String get ports => 'Reenvío de puertos';

  @override
  String get browser => 'Vista previa web';

  @override
  String get portsSub => '1 delantero';

  @override
  String get connectionOwner => 'Ejecución Node · Estudio';

  @override
  String get openTerminal => 'Abrir la terminal';

  @override
  String get tables => 'Cuadros';

  @override
  String get portNote => 'Sin escucha de puerto local';

  @override
  String get portTarget => 'Puerto de destino';

  @override
  String get localPort => 'El puerto local';

  @override
  String get closePort => 'Cerrar hacia adelante';

  @override
  String get portClosed => 'Muestra hacia adelante cerrada';

  @override
  String get diffSubtitle => 'sailry-web · feature/sign-in';

  @override
  String get workingTree => 'Árbol de trabajo';

  @override
  String get staged => 'Preparados';

  @override
  String get diffSummary => '3 filas';

  @override
  String get stage => 'Todas las etapas';

  @override
  String get unstage => 'Unstage';

  @override
  String get commit => 'Commit';

  @override
  String get commitTitle => 'Confirmar los cambios';

  @override
  String get commitMessage => 'Mensaje de confirmación';

  @override
  String get commitPlaceholder => 'Describa los cambios';

  @override
  String get commitPreview => 'Simular commit';

  @override
  String get committed => 'Sample commit completado';

  @override
  String get noChanges => 'No hay cambios que confirmar';

  @override
  String get diffFile => 'login.css';

  @override
  String get diffPath => 'src/styles/login.css';

  @override
  String get continueEdit => 'Continúa la edición';

  @override
  String get diffSelection => 'Elegir archivo cambiado';

  @override
  String get terminalKeyboard => 'Teclado';

  @override
  String get terminalEnter => 'Intro';

  @override
  String get terminalOutputLabel => 'Salida del terminal';

  @override
  String get terminalSubtitle => 'Studio / sailry-web';

  @override
  String get readOnly => 'Solo lectura';

  @override
  String get takeControl => 'Tome el control';

  @override
  String get hasControl => 'Control de entrada';

  @override
  String get releaseControl => 'Control de liberación';

  @override
  String get terminalPlaceholder => 'Introduzca un comando de ejemplo';

  @override
  String get terminalPreview =>
      'Ejemplo de terminal · Los comandos no se ejecutan';

  @override
  String get terminalOutput => 'Comando recibido en vista previa, no ejecutado';

  @override
  String get terminalControlNote =>
      'Tome el control para enviar entrada; simulado aquí';

  @override
  String get usageSubtitle => 'Sailry chats solamente';

  @override
  String get week => 'Esta semana';

  @override
  String get month => 'Este mes';

  @override
  String get tokens => 'Tokens';

  @override
  String get requests => 'Responses';

  @override
  String usageCoverage(String priced, String total) {
    return '$priced / $total respuestas con precio';
  }

  @override
  String get usageEmpty => 'Sin datos de uso';

  @override
  String get estimatedCost => 'Costo estimado';

  @override
  String get costCoverage => '42 / 48 respuestas con precio';

  @override
  String get partial => 'Datos parciales';

  @override
  String get sourcesPartial => '2 / 3 hosts actualizados';

  @override
  String get input => 'Entrada';

  @override
  String get output => 'Salida';

  @override
  String get cached => 'Hits de caché';

  @override
  String get modelUsage => 'Distribución del modelo';

  @override
  String get hostUsage => 'Uso del host';

  @override
  String get recentRequests => 'Respuestas recientes';

  @override
  String get allUsage => 'Detalles de uso';

  @override
  String get usageNote =>
      'Los costos son estimados; algunas respuestas no tienen precio';

  @override
  String get sourceNote =>
      'Los hosts sin conexión conservan sus últimos datos conocidos';

  @override
  String get profileSubtitle => 'Controlador de móvil';

  @override
  String get localSettings => 'Las preferencias locales';

  @override
  String get nodeSettings => 'Ejecución Node ajustes';

  @override
  String get appearance => 'Apariencia';

  @override
  String get notifications => 'Notificaciones';

  @override
  String get enabled => 'Activado';

  @override
  String get disabled => 'Desactivado';

  @override
  String get add => 'Añadir';

  @override
  String get configName => 'Nombre';

  @override
  String get configEndpoint => 'Punto final';

  @override
  String get configModels => 'Modelos';

  @override
  String get configInstructions => 'Instrucciones';

  @override
  String get configContent => 'Contenido';

  @override
  String get configEmpty => 'No hay entradas';

  @override
  String get configDuplicate => 'El nombre ya existe';

  @override
  String configDelete(String name) {
    return '¿Borrar “$name”?';
  }

  @override
  String get speechInput => 'Entrada de voz';

  @override
  String get developerInstructions =>
      'Cambiar el código de la tarea y verificar el resultado';

  @override
  String get reviewerInstructions =>
      'Revisar cambios de código e identificar problemas';

  @override
  String get projectConventions => 'Convenios de proyectos';

  @override
  String get memoryContent => 'Conservar el estilo de código existente';

  @override
  String get providers => 'Modelos y proveedores';

  @override
  String get roles => 'Funciones';

  @override
  String get memorySettings => 'Memoria';

  @override
  String get speech => 'Discurso';

  @override
  String nodeSettingsNote(String host) {
    return 'Configuración guardada en $host';
  }

  @override
  String get about => 'Acerca de Sailry';

  @override
  String get aboutBody =>
      'Vista previa de interacción móvil, no conectada a servicios';

  @override
  String get settingsSaved => 'Configuración actualizada en esta vista previa';

  @override
  String get modelPicker => 'Elige el modelo';

  @override
  String get nodeDefaults => 'Node por defecto';

  @override
  String get providerNote =>
      '· Las credenciales permanecen en la ejecución Node';

  @override
  String get roleNote => 'Ejemplo de rol · Se aplica a los nuevos chats';

  @override
  String get auto => 'Automático';

  @override
  String get manual => 'Pregunta cada vez';

  @override
  String get notificationsNote =>
      'Controla solo las notificaciones de vista previa';

  @override
  String get memoryNote => 'Muestra Node memoria';

  @override
  String get speechNote => 'Utiliza la configuración de voz de ejecución Node';

  @override
  String get newTaskHost => 'Host de ejecución';

  @override
  String get newTaskProject => 'Proyecto';

  @override
  String get newTaskWorktree => 'Árbol de trabajo';

  @override
  String get create => 'Crear';

  @override
  String get taskCreated => 'Chat de muestra creado';

  @override
  String get required => 'Describa su tarea primero';

  @override
  String get notificationsEmpty => 'No hay nuevas notificaciones';

  @override
  String get reviewTitle => 'Referencias de diseño';

  @override
  String get reviewIntro =>
      'Las páginas siguen la fuente actual; esta vista previa no establece la aceptación del servicio móvil';

  @override
  String get reviewConversation => 'Chats, aprobaciones, preguntas y cola';

  @override
  String get reviewConversationText =>
      'Las tareas conservan la propiedad de host, proyecto y árbol de trabajo; amplíe los registros de herramientas y las aprobaciones dentro de los chats';

  @override
  String get reviewResources => 'Fichas, Git, terminales y conexiones';

  @override
  String get reviewResourcesText =>
      'La edición de archivos, la preparación, las confirmaciones y los ports conservan sus puntos de entrada; los detalles se abren en páginas secundarias';

  @override
  String get reviewHosts => 'Conexiones de host y monitoreo';

  @override
  String get reviewHostsText =>
      'Nodos emparejados, códigos de 6 dígitos, uso de recursos y procesos; el estado sin conexión no se muestra como activo';

  @override
  String get reviewUsage => 'Uso y configuración de Node';

  @override
  String get reviewUsageText =>
      'Sailry solo chats; el uso agregado conserva la integridad y los costos indican estimaciones y cobertura';

  @override
  String get reviewBoundary => 'Límite móvil';

  @override
  String get reviewBoundaryText =>
      'El puente móvil expone conexiones, chats, terminales y uso; esta vista previa comienza no Node, modelos, emparejamiento, terminales o plugins';

  @override
  String get reviewVisual => 'Referencias visuales';

  @override
  String get reviewVisualText =>
      'Referencia 1: jerarquía de chat; referencia 2: tarjetas flexibles y navegación flotante; referencia 3: monitoreo compacto';

  @override
  String get hostConnectPrompt => 'Conectar un host';

  @override
  String get hostDisconnected => 'Se perdió la conexión';

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
  String get backgroundConnection => 'Manténgase conectado en segundo plano';

  @override
  String get backgroundConnectionActive =>
      'Mantener las conexiones de host activas';

  @override
  String get backgroundConnectionFailed =>
      'Conexión en segundo plano no habilitada; vuelva a intentarlo';

  @override
  String get resetReasoning => 'Restablecer el esfuerzo';

  @override
  String get completionAlerts => 'Alertas de finalización';

  @override
  String get notificationsReadAll => 'Marcar todo como leído';

  @override
  String get notificationsOpen => 'Abrir';

  @override
  String get preferencesFailed =>
      'Preferencias no guardadas; vuelva a intentarlo';

  @override
  String get connectFirst => 'Conecte un host para comenzar';

  @override
  String get initializing => 'Iniciando';

  @override
  String get startupFailed => 'Falló el inicio';

  @override
  String get retryConnection => 'Reintentar';

  @override
  String get pairAction => 'Conectar';

  @override
  String get pairFailed => 'Falló la conexión; vuelva a intentarlo';

  @override
  String get pairExpired =>
      'El código de emparejamiento ha caducado; obtén un código nuevo';

  @override
  String get pairing => 'Conectando';

  @override
  String get hostUnavailable => 'Host no conectado';

  @override
  String get hostMetricsFailed => 'No se puede leer el estado del host';

  @override
  String get hostProcessesEmpty => 'No hay procesos';

  @override
  String get hostRegisterProject => 'Añadir proyecto »';

  @override
  String get hostChooseDirectory => 'Elige el directorio';

  @override
  String get hostChooseFile => 'Elige el archivo';

  @override
  String get hostParentDirectory => 'Directorio superior';

  @override
  String get hostEmptyDirectory => 'El directorio está vacío';

  @override
  String get hostLoadMore => '» Cargar más';

  @override
  String get hostProjectName => 'Nombre del proyecto';

  @override
  String get hostProjectPath => 'Ruta del proyecto en el host';

  @override
  String get hostProjectFailed => 'No se puede agregar proyecto';

  @override
  String get hostUnknown => 'No hay datos';

  @override
  String get hostRefresh => 'Actualizar';

  @override
  String get hostMetricCpu => 'CPU';

  @override
  String get hostMetricMemory => 'Memoria';

  @override
  String get hostMetricDisk => 'Disco';

  @override
  String get failureConflict =>
      'Contenido cambiado; recarga e inténtalo de nuevo';

  @override
  String get failureUnknown =>
      'Resultado no confirmado; compruebe primero el estado del host';

  @override
  String get failureDenied => 'Permiso denegado';

  @override
  String get failureUnavailable => 'Host no conectado';

  @override
  String get failureBusy => 'Vuelve a intentarlo más tarde';

  @override
  String get failureGeneric => 'La operación falló';

  @override
  String get settingsSpeechLanguage => 'Idioma';

  @override
  String get settingsSpeechAuto => 'Detectar automáticamente';

  @override
  String get settingsSpeechChinese => 'Chino';

  @override
  String get settingsSpeechEnglish => 'English';

  @override
  String get settingsSpeechReady => 'Modelo de discurso listo';

  @override
  String get settingsSpeechDownload => 'Descargar modelo de discurso';

  @override
  String get settingsSpeechFailed =>
      'El modelo de voz no está listo; vuelva a intentarlo';

  @override
  String get settingsNoHost => 'Conectar un host primero';

  @override
  String get settingsUnavailable => 'No disponible';

  @override
  String get settingsLoadFailed => 'No se pudo cargar';

  @override
  String get settingsSaveFailed => 'Fallo al guardar; borrador conservado';

  @override
  String get settingsConflict =>
      'Configuración cambiada; vuelva a abrir y vuelva a intentarlo';

  @override
  String get settingsUnknown =>
      'Resultado no confirmado; actualizar para comprobar';

  @override
  String get settingsRetry => 'Reintentar';

  @override
  String get settingsLoading => 'Cargando';

  @override
  String get settingsRequired => 'Introduzca un valor';

  @override
  String get settingsKey => 'ID';

  @override
  String get settingsDescription => 'Descripción';

  @override
  String get settingsInstructions => 'Instrucciones';

  @override
  String get settingsModels => 'ID de modelo, uno por línea';

  @override
  String get settingsApi => 'API formato';

  @override
  String get settingsOpenCodeGo => 'OpenCode Go';

  @override
  String get settingsOpenCodeZen => 'OpenCode Zen';

  @override
  String get settingsApiKey => 'Clave de API';

  @override
  String get settingsEnabled => 'Activado';

  @override
  String get settingsDriver => 'Servicio';

  @override
  String get settingsModel => 'Modelo';

  @override
  String get settingsMemoryAuto => 'Grabar automáticamente';

  @override
  String get settingsMemoryBudget => 'Bytes de contexto';

  @override
  String get settingsMemoryReview => 'Intervalo de revisión (días)';

  @override
  String get settingsMemoryRecords => 'Entradas de memoria';

  @override
  String get settingsMemoryKind => 'Tipo';

  @override
  String get settingsMemoryUser => 'Usuario';

  @override
  String get settingsMemoryFeedback => 'Comentarios';

  @override
  String get settingsMemoryProject => 'Proyecto';

  @override
  String get settingsMemoryReference => 'Referencia';

  @override
  String get settingsArchived => 'Archivado';

  @override
  String get settingsEmpty => 'No hay registros';

  @override
  String get settingsUsageUnknown => 'Desconocido';

  @override
  String get settingsUsagePartial => 'Algunos hosts no están disponibles';

  @override
  String get settingsUsageCache => 'En caché';

  @override
  String get settingsUsageInput => 'Entrada sin caché';

  @override
  String get settingsUsageOutput => 'Salida';

  @override
  String get settingsUsageDaily => 'Diariamente';

  @override
  String get settingsUsageWeekly => 'Semanal';

  @override
  String get settingsUtc => 'UTC';
}
