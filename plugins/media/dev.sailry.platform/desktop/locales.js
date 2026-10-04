// Settings captions from Sailry 5d0ac251, localized for the package.
const en = {
  media_roles:'Media model roles',
  media_unconfigured:'Unconfigured',
  media_unavailable:'Unavailable',
  media_model_unavailable:'Model unavailable',
  media_conflict:'Settings changed; select again',
  media_unknown:'Save outcome is unknown; check the current settings',
  media_failed:'Save failed',
  media_image_understanding:'Image understanding',
  media_image_generation:'Image generation',
  media_audio_transcription:'Audio transcription',
  media_speech_synthesis:'Speech synthesis',
  media_video_understanding:'Video understanding',
  media_video_generation:'Video generation'
};
const zh = {
  media_roles:'媒体模型用途',
  media_unconfigured:'未配置',
  media_unavailable:'暂未接入',
  media_model_unavailable:'模型不可用',
  media_conflict:'配置已变更，请重新选择',
  media_unknown:'保存结果未确认，请查看当前配置',
  media_failed:'保存失败',
  media_image_understanding:'图片理解',
  media_image_generation:'图片生成',
  media_audio_transcription:'音频转写',
  media_speech_synthesis:'语音合成',
  media_video_understanding:'视频理解',
  media_video_generation:'视频生成'
};
export function messages(locale) { return locale === 'zh-CN' ? zh : en; }
