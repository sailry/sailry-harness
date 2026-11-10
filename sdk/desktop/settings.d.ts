export function loadSettingSecret(secret:string,field:string,expectedRevision:string):Promise<boolean>;
export function prepareSettings(values:Record<string,import('sailry/sdk').Json>,edits:Record<string,{action:'keep'|'clear'}|{action:'set';secret:string}>,expectedRevision:string):string;
