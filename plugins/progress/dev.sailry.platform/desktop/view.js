// Accepted four-lane workbench layout; Kit owns controls, focus and scrolling.
import {div} from 'gpui-kit';
import {Icon} from 'gpui-component';
import {theme} from 'sailry';
import {EmptyState,Appearance,CardButton,ClampedText,HoverSwap,ThinkingIcon,ScrollRegion,pageInsets,Collapse,CollapseSlot} from 'sailry/ui';
import {lanes,labels,sections,clock} from './state.js';
const glyphs = {waiting:'progress-4-line',running:'progress-2-line',completed:'progress-6-line',
  failed:'close-circle-line',idle:'checkbox-blank-circle-line'};
const tone = (lane,colors) => lane === 'waiting' ? colors.warning : lane === 'running' ? colors.primary
  : lane === 'completed' ? colors.success : lane === 'failed' ? colors.destructive : colors.muted_foreground;
const scrollKey = (view,lane,project) => `${view.board.catalog.node}-activity-project-${labels[lane]}-${project ?? 'none'}-cards`;

function card(view,item) {
  const {text} = view, colors = theme().colors;
  const id = item.kind === 'terminal' ? `activity-terminal-${item.id}` : `activity-${item.id}`;
  view.targets.set(id,{kind:item.kind,id:item.id});
  return CardButton.new(id,{label:`${item.title}, ${item.status}, ${item.project}`,tooltip:item.status})
    .child(div().v_flex().w_full().min_w_0().items_start().gap_2()
      .child(div().id(`${id}-header`).h_flex().w_full().min_w_0().justify_between().gap_2()
        .children(item.phase ? [ThinkingIcon.new(`${id}-loading`,{phase:item.phase})] : [])
        .child(div().id(`${id}-title`).flex_1().min_w_0().h_5().line_height('1.25rem').text_left().text_sm().font_semibold().truncate().child(item.title))
        .child(HoverSwap.new(`${id}-metadata`,{group:id})
          .child(div().id(`${id}-location`).h_flex().w_full().h_4().justify_end().line_height('1rem')
            .text_left().text_xs().font_normal().text_color(colors.muted_foreground).min_w_0().gap_1().opacity(0.7)
            .child(div().id(`${id}-project-icon`).size_4().flex_shrink(0).child(Appearance.new(`${id}-appearance`,{value:{...item.appearance,color:'none'}})))
            .child(div().id(`${id}-project`).min_w_0().truncate().child(item.project || text.sessions_unassigned)))
          .children(item.time === null ? [] : [div().id(`${id}-time`).h_flex().w_full().h_4().justify_end().line_height('1rem')
            .text_xs().font_normal().text_color(colors.muted_foreground).gap_1().opacity(0.7)
            .child(new Icon('reicon:time/clock').size('xsmall'))
            .child(clock(item.time))])))
      .child(ClampedText.new(`${id}-content`,{text:item.content,lines:2,danger:item.lane === 'failed'})));
}

function project(view,lane,section) {
  const colors = theme().colors;
  const selector = `activity-project-${labels[lane]}-${section.id ?? 'none'}`;
  const identity = scrollKey(view,lane,section.id);
  return Collapse.new(`${view.board.catalog.node}-${selector}`,{label:section.name,default_open:true,variant:'plain'})
    .child(CollapseSlot.new(selector,{variant:'header'})
      .child(div().h_flex().w_full().min_w_0().gap_2()
        .child(Appearance.new(`${selector}-appearance`,{value:section.appearance}))
        .child(div().flex_1().min_w_0().truncate().text_left().text_sm().child(section.name))
        .child(div().text_xs().text_color(colors.muted_foreground).child(String(section.cards.length)))))
    .child(CollapseSlot.new(`${selector}-body`,{variant:'content'})
      .child(div().v_flex().w_full().min_w_0().pt_2()
        .child(div().id(`${selector}-viewport`).relative().w_full().min_w_0()
          // Natural height for short groups; the retained native region chains at its edges.
          .child(ScrollRegion.new(identity,{max_height:window.rem_size() * 32.5})
            .children(section.cards.map(item => card(view,item)))))));
}

export function render(view,cx) {
  const {board,text} = view, colors = theme().colors, rem = window.rem_size();
  const columns = sections(board.catalog,board.previews,text);
  const available = window.viewport_size().width - pageInsets().rail_width - rem * 2 - rem * 0.75 * 3;
  const width = Math.max(240,available / 4);
  view.targets = new Map();
  return div().v_flex().size_full().min_w_0().gap_4().p_4()
    .child(div().id('activity-board').h_flex().w_full().items_start().flex_1().min_h_0().gap_3().overflow_x_scrollbar()
      .children(lanes.map(lane => {
        const key = labels[lane], groups = columns[lane], count = groups.reduce((sum,section) => sum + section.cards.length,0);
        return div().id(`${key}-column`).v_flex().flex_none().w(width).h_full().gap_3()
          .child(div().id(`${key}-heading`).h_flex().w_full().gap_2().px_4().py_3().rounded(cx.theme().radius.lg).bg(colors.group_box).border_1().border_color(colors.border)
            .child(new Icon(`icons/remix/${glyphs[lane]}.svg`).size('small').color(tone(lane,colors)))
            .child(div().flex_1().font_semibold().text_sm().child(text[key]))
            .child(div().text_xs().px_2().rounded_md().bg(colors.muted).text_color(colors.muted_foreground).child(String(count))))
          .child(div().id(`${key}-items`).v_flex().flex_1().min_h_0().min_w_0().gap_3().overflow_y_scrollbar()
            .children(count === 0 ? [EmptyState.new(`${key}-empty`,{variant:'list',fill_height:true,vertical_align:'start',icon:'inbox',label:text.activity_board_empty})] : [])
            .children(groups.map(section => project(view,lane,section))));
      })));
}
