// Web Worker: all computation runs here, off the main thread.
// Sends real intermediate data back for display.

function aboCompat(d,r){return d===0||d===r||r===3;}
function pickBT(rng){const r=rng();return r<.3?0:r<.7?1:r<.9?2:3;}
function makeRng(seed){let s=BigInt(seed);return()=>{s=(s*6364136223846793005n+1442695040888963407n)&0xFFFFFFFFFFFFFFFFn;return Number(s>>33n)/2147483648;};}
const BT_NAMES=['O','A','B','AB'];

function gen(nd,nr,seed){
  const rng=makeRng(seed),L=[0,100,250,400,550,700,850,1000],ds=[],rs=[];
  for(let i=0;i<nd;i++)ds.push({id:`D${String(i).padStart(3,'0')}`,bt:pickBT(rng),lv:Math.round(1200+rng()*600),km:Math.round(L[Math.floor(rng()*8)%8]+(rng()-.5)*100)});
  for(let i=0;i<nr;i++)rs.push({id:`R${String(i).padStart(3,'0')}`,bt:pickBT(rng),meld:Math.round(10+rng()*30),bw:Math.round(45+rng()*40),km:Math.round(L[Math.floor(rng()*8)%8]+(rng()-.5)*100),wd:Math.round(rng()*1500)});
  return{donors:ds,recipients:rs};
}

// Simulated FHE encryption (XOR with key, shows real hex output)
function encrypt(data, key){
  const bytes = new TextEncoder().encode(JSON.stringify(data));
  const enc = new Uint8Array(bytes.length);
  for(let i=0;i<bytes.length;i++) enc[i] = bytes[i] ^ key[i % key.length];
  return Array.from(enc).map(b=>b.toString(16).padStart(2,'0')).join('');
}

function generateKey(id){
  let hash = 0;
  for(let i=0;i<id.length;i++) hash = ((hash<<5)-hash+id.charCodeAt(i))|0;
  const key = new Uint8Array(32);
  for(let i=0;i<32;i++){hash=((hash*1103515245+12345)&0x7fffffff);key[i]=hash&0xff;}
  return key;
}

// ZKP proof generation (simulated but produces real bytes)
function generateProof(donorBt, recipBt, score, isCompat){
  const proofData = new Uint8Array(48);
  // Header: "argo-zkp-v1"
  const header = new TextEncoder().encode('argo-zkp-v1:');
  proofData.set(header);
  proofData[12] = isCompat ? 1 : 0;
  proofData[13] = score > 0.7 ? 3 : score > 0.3 ? 2 : score > 0 ? 1 : 0;
  // Commitment bytes (simulated Pedersen commitment)
  const rng = makeRng(donorBt * 1000 + recipBt * 100 + Math.round(score * 1000));
  for(let i=14;i<48;i++) proofData[i] = Math.floor(rng()*256);
  return Array.from(proofData).map(b=>b.toString(16).padStart(2,'0')).join('');
}

function sc(d,r,mw){
  if(!aboCompat(d.bt,r.bt))return 0;
  const g=d.lv/r.bw/10;
  if(g<.8||g>5)return 0;
  return .35*Math.max(0,Math.min(1,(r.meld-6)/34))+.25*Math.max(0,1-Math.abs(g-2)/3)+.25*(Math.abs(d.km-r.km)>1200?0:1-Math.abs(d.km-r.km)/1200)+.15*(mw>0?Math.min(1,r.wd/mw):0);
}

function buildS(ds,rs){const mw=Math.max(...rs.map(r=>r.wd));return ds.map(d=>rs.map(r=>sc(d,r,mw)));}

function greedy(s){
  const nd=s.length,nr=s[0].length;let c=[];
  for(let d=0;d<nd;d++)for(let r=0;r<nr;r++)if(s[d][r]>0)c.push([d,r,s[d][r]]);
  c.sort((a,b)=>b[2]-a[2]);const md=new Set,mr=new Set,res=[];
  for(const[d,r,v]of c)if(!md.has(d)&&!mr.has(r)){md.add(d);mr.add(r);res.push([d,r,v]);}
  return res;
}

function sa(s,sw){
  const nd=s.length,nr=s[0].length,P=10,vs=[],vm=[];
  for(let d=0;d<nd;d++)for(let r=0;r<nr;r++)if(s[d][r]>0){vs.push(-s[d][r]);vm.push([d,r]);}
  const n=vs.length;if(!n)return{pairs:[],sweepsDone:0};
  const nb=Array.from({length:n},()=>[]);
  for(let i=0;i<n;i++)for(let j=i+1;j<n;j++)if(vm[i][0]===vm[j][0]||vm[i][1]===vm[j][1]){nb[i].push([j,P]);nb[j].push([i,P]);}
  const rng=makeRng(42);let st=Array.from({length:n},()=>rng()<.3),e=0;
  for(let i=0;i<n;i++)if(st[i])e+=vs[i];
  for(let i=0;i<n;i++)if(st[i])for(const[j,v]of nb[i])if(j>i&&st[j])e+=v;
  let b=[...st],be=e;const tR=Math.log(.001/10);
  const reportEvery=Math.max(1,Math.floor(sw/50));

  for(let w=0;w<sw;w++){
    const t=10*Math.exp(tR*w/sw);
    for(let k=0;k<n;k++){
      const f=Math.floor(rng()*n)%n;
      let d=st[f]?-vs[f]:vs[f];
      for(const[x,v]of nb[f])if(st[x])d+=st[f]?-v:v;
      if(d<0||rng()<Math.exp(-d/t)){st[f]=!st[f];e+=d;if(e<be){be=e;b=[...st];}}
    }
    if(w%reportEvery===0){
      const currentPairs=b.filter(v=>v).length;
      self.postMessage({type:'sa_progress',sweep:w,total:sw,pct:Math.round(w/sw*100),energy:be.toFixed(4),currentMatches:currentPairs,temp:t.toFixed(4)});
    }
  }
  const pairs=b.map((v,i)=>v?vm[i]:null).filter(x=>x);
  return{pairs,energy:be};
}

self.onmessage = function(ev) {
  const {nd, nr, sweeps, seed} = ev.data;

  // === Step 1: Key generation ===
  const keys = [];
  for(let i=0;i<nd+nr;i++){
    const id = i < nd ? `anon-d${String(i).padStart(3,'0')}` : `anon-r${String(i-nd).padStart(3,'0')}`;
    const key = generateKey(id);
    keys.push({id, keyHex: Array.from(key).map(b=>b.toString(16).padStart(2,'0')).join('')});
  }
  self.postMessage({type:'keys', keys: keys.slice(0,6), total: keys.length});

  // === Step 2: Generate + Encrypt ===
  const {donors, recipients} = gen(nd, nr, seed);
  const encryptedSamples = [];
  // Show a few encryption examples
  for(let i=0;i<Math.min(3,nd);i++){
    const d = donors[i];
    const plain = {blood_type:BT_NAMES[d.bt], liver_volume_ml:d.lv, region_km:d.km};
    const key = generateKey(`anon-d${String(i).padStart(3,'0')}`);
    const cipher = encrypt(plain, key);
    encryptedSamples.push({id:`anon-d${String(i).padStart(3,'0')}`, plain, cipher});
  }
  for(let i=0;i<Math.min(2,nr);i++){
    const r = recipients[i];
    const plain = {blood_type:BT_NAMES[r.bt], meld:r.meld, body_weight_kg:r.bw, waiting_days:r.wd};
    const key = generateKey(`anon-r${String(i).padStart(3,'0')}`);
    const cipher = encrypt(plain, key);
    encryptedSamples.push({id:`anon-r${String(i).padStart(3,'0')}`, plain, cipher});
  }
  self.postMessage({type:'encrypted', samples: encryptedSamples, totalRecords: nd+nr});

  // === Step 3: Score matrix ===
  const t0s = performance.now();
  const scores = buildS(donors, recipients);
  const scoreTime = performance.now() - t0s;
  const nCompat = scores.flat().filter(v=>v>0).length;
  // Show score samples
  const scoreSamples = [];
  for(let d=0;d<Math.min(5,nd);d++)
    for(let r=0;r<Math.min(5,nr);r++)
      if(scores[d][r]>0)
        scoreSamples.push({donor:`D${String(d).padStart(3,'0')}`,recip:`R${String(r).padStart(3,'0')}`,score:scores[d][r].toFixed(4)});
  self.postMessage({type:'scored', nCompat, scoreTime, totalPairs:nd*nr, scoreSamples:scoreSamples.slice(0,8)});

  // === Step 4: ZKP proofs ===
  const proofSamples = [];
  let proofCount = 0;
  for(let d=0;d<nd;d++){
    for(let r=0;r<nr;r++){
      if(scores[d][r]>0){
        proofCount++;
        if(proofSamples.length<4){
          const proofHex = generateProof(donors[d].bt, recipients[r].bt, scores[d][r], true);
          proofSamples.push({
            donor:`anon-d${String(d).padStart(3,'0')}`,
            recip:`anon-r${String(r).padStart(3,'0')}`,
            compatible:true,
            bucket:scores[d][r]>0.7?'HIGH':scores[d][r]>0.3?'MED':'LOW',
            proofHex: proofHex.substring(0,64)+'...',
            proofBytes: proofHex.length/2,
          });
        }
      }
    }
  }
  self.postMessage({type:'proofs', proofCount, proofSamples});

  // === Step 5: Greedy ===
  const t0g = performance.now();
  const gResult = greedy(scores);
  const greedyTime = performance.now() - t0g;
  const greedyScore = gResult.reduce((a,x)=>a+x[2],0);
  self.postMessage({type:'greedy_done', matches:gResult.length, score:greedyScore, time:greedyTime});

  // === Step 5b: Quantum (SA) — with progress updates ===
  const t0q = performance.now();
  const {pairs:qPairs, energy} = sa(scores, sweeps);
  const quantumTime = performance.now() - t0q;
  const quantumScore = qPairs.reduce((a,[d,r])=>a+scores[d][r],0);

  // === Step 6: Results ===
  const matchDetails = qPairs.map(([d,r])=>({
    donor:`anon-d${String(d).padStart(3,'0')}`,
    recip:`anon-r${String(r).padStart(3,'0')}`,
    score:scores[d][r].toFixed(4),
    donorBt:BT_NAMES[donors[d].bt],
    recipBt:BT_NAMES[recipients[r].bt],
  }));

  self.postMessage({
    type:'done',
    nCompat,
    greedyMatches:gResult.length, greedyScore, greedyTime,
    quantumMatches:qPairs.length, quantumScore, quantumTime, energy,
    diff:qPairs.length-gResult.length,
    matchDetails,
  });
};
