/** Native credential handles never expose secret contents to JavaScript. */
export function prepareSettings(input:{settings:import('sailry/sdk').Json;driver:string;key:{action:'keep'|'clear'}|{action:'set';secret:string};basic:{action:'keep'|'clear'}|{action:'set';username:string;password:string}}):string;
