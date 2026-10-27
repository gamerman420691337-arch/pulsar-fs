// Authored diagnostic graph, not a trained detector or quality benchmark.
// Wire fields: https://github.com/onnx/onnx/blob/main/onnx/onnx.proto3
// Input-dependent score: 0.6 + 0.1 * mean(RGB). Fixed synthetic box.
export function authoredOnnxFixture({temporal=false,external=false,twoTargets=false}={}) {
  const join = (...parts) => Buffer.concat(parts.flat());
  const varint = value => {
    let n = BigInt(value), bytes = [];
    do { let byte = Number(n & 127n); n >>= 7n; bytes.push(byte | (n ? 128 : 0)); } while (n);
    return Buffer.from(bytes);
  };
  const integer = (field,value) => join(varint(field*8),varint(value));
  const message = (field,bytes) => join(varint(field*8+2),varint(bytes.length),bytes);
  const string = (field,value) => message(field,Buffer.from(value));
  const shape = dims => join(dims.map(dim=>message(1,integer(1,dim))));
  const info = (name,dims) => join(string(1,name),message(2,message(1,join(integer(1,1),message(2,shape(dims))))));
  const tensor = (name,values,dims=[1,5,1]) => {
    const raw = Buffer.alloc(values.length*4);
    values.forEach((value,index)=>raw.writeFloatLE(value,index*4));
    return join(dims.map(dim=>integer(1,dim)),integer(2,1),string(8,name),external ? join(message(13,join(string(1,'location'),string(2,'../outside.weights'))),integer(14,1)) : message(9,raw));
  };
  const node = (op,inputs,output,attrs=[]) => join(inputs.map(input=>string(1,input)),string(2,output),string(4,op),attrs.map(attr=>message(5,attr)));
  const keepdims = join(string(1,'keepdims'),integer(3,0),integer(20,2));
  if(temporal){
    const graph=join(message(1,node('ReduceMean',['frame_pair'],'mean',[keepdims])),message(1,node('Mul',['mean','gain'],'delta')),message(1,node('Add',['delta','base'],'script_position')),string(2,'pulsar-authored-temporal-fixture'),message(5,tensor('gain',[0.5],[1,1])),message(5,tensor('base',[0.25],[1,1])),message(11,info('frame_pair',[1,6,64,64])),message(12,info('script_position',[1,1])));
    return join(integer(1,8),string(2,'pulsar-authored-temporal-fixture'),message(7,graph),message(8,integer(2,13)));
  }
  const graph = join(
    message(1,node('ReduceMean',['rgb'],'mean',[keepdims])),
    message(1,node('Mul',['mean','gain'],'delta')),
    message(1,node('Add',['delta','base'],'boxes')),
    string(2,'pulsar-authored-input-dependent-fixture'),
    message(5,tensor('gain',twoTargets?[0,0,0,0,0,0,0,0,0.1,0.1]:[0,0,0,0,0.1],twoTargets?[1,5,2]:[1,5,1])),
    message(5,tensor('base',twoTargets?[20,44,32,32,16,16,28,28,0.6,0.9]:[32,32,20,20,0.6],twoTargets?[1,5,2]:[1,5,1])),
    message(11,info('rgb',[1,3,64,64])),message(12,info('boxes',twoTargets?[1,5,2]:[1,5,1])));
  return join(integer(1,8),string(2,'pulsar-authored-fixture'),message(7,graph),message(8,integer(2,13)));
}
