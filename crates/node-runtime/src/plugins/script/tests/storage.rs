use super::*;

#[test]
fn classifies_only_atomic_value_conflicts() {
    let output=execute(r#"
import {isValueConflict} from 'sailry/sdk';
export function run() {
  const error={code:'revision_conflict',message:'plugin value changed',details:{kind:'plugin_value_conflict'}};
  return [isValueConflict({isError:true,error}),isValueConflict({isError:false,error}),
    ...['revision_conflict','outcome_unknown','cancelled','permission_denied','unavailable','internal']
      .map(code=>isValueConflict({isError:true,error:{code,message:'original'}})),
    isValueConflict({isError:true,error:{...error,code:'outcome_unknown'}})];
}
"#,CancellationToken::new(),CALL_TIMEOUT).unwrap();
    assert_eq!(
        output,
        json!([true, false, false, false, false, false, false, false, false])
    );
}
