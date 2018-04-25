#![feature(proc_macro, wasm_custom_section, wasm_import_module)]
#![feature(iterator_step_by)]

extern crate byteorder;
extern crate wasm_bindgen;

use byteorder::{LittleEndian, ReadBytesExt};

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    // fn log(s1: i8);
    // fn log(s1: i32, s2: i32);
    fn log(s1: i32, s2: i32, s3: f32, s4: f32);
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_string(a: &str);
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_one(a: f32);
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_two(a: isize, b: isize);
}

#[wasm_bindgen]
pub struct Image {
    ldim: i32,
    ncols: usize,
    nrows: usize,
    pixels: Vec<u8>,
}

#[wasm_bindgen]
impl Image {
    pub fn new(ldim: i32, ncols: usize, nrows: usize, pixels: Vec<u8>) -> Image {
        Image {
            ldim,
            ncols,
            nrows,
            pixels,
        }
    }
}

#[wasm_bindgen]
pub struct RunParams {
    max_size: f32,
    min_size: f32,
    scale_factor: f32,
    shift_factor: f32,
}

#[wasm_bindgen]
impl RunParams {
    pub fn new(max_size: f32, min_size: f32, scale_factor: f32, shift_factor: f32) -> RunParams {
        RunParams {
            max_size,
            min_size,
            scale_factor,
            shift_factor,
        }
    }
}

struct Detection(i32, i32, f32, f32);
struct Cluster(f32, f32, f32, f32);

#[wasm_bindgen]
pub struct Pico {
    tdepth: i32,
    ntrees: i32,
    tdepth_sqr: isize,
    tcodes: Vec<u8>,
    tpreds: Vec<f32>,
    thresh: Vec<f32>,
    detections: Vec<Detection>,
}

#[wasm_bindgen]
impl Pico {
    pub fn new() -> Pico {
        Pico {
            tdepth: 0,
            ntrees: 0,
            tdepth_sqr: 0,
            tcodes: Vec::new(),
            tpreds: Vec::new(),
            thresh: Vec::new(),
            detections: Vec::new(),
        }
    }

    pub fn unpack_cascade(&mut self, bytes: Vec<u8>) {
        let mut p = 8;
        let mut buff = &bytes[p..p + 4];
        self.tdepth = buff.read_i32::<LittleEndian>().unwrap();
        self.tdepth_sqr = (2 as isize).pow(self.tdepth as u32);
        p = p + 4;

        let mut buff = &bytes[p..p + 4];
        self.ntrees = buff.read_i32::<LittleEndian>().unwrap();
        p = p + 4;

        for _ in 0..self.ntrees {
            self.tcodes.extend_from_slice(&[0, 0, 0, 0]);
            let next_p = p + 4 * self.tdepth_sqr as usize - 4;
            self.tcodes.extend_from_slice(&bytes[p..next_p]);
            p = next_p;

            for _ in 0..self.tdepth_sqr {
                let mut buff = &bytes[p..p + 4];
                self.tpreds.push(buff.read_f32::<LittleEndian>().unwrap());
                p = p + 4;
            }

            let mut buff = &bytes[p..p + 4];
            self.thresh.push(buff.read_f32::<LittleEndian>().unwrap());
            p = p + 4;
        }
    }

    pub fn run_cascade(&mut self, image: &Image, params: &RunParams) {
        let Image {
            ldim,
            ncols,
            nrows,
            pixels,
        } = image;

        let RunParams {
            max_size,
            min_size,
            scale_factor,
            shift_factor,
        } = params;

        let mut scale = min_size.clone();
        self.detections = Vec::new();

        while scale <= *max_size {
            let step = (1 as f32).max(shift_factor * scale) as i32;
            let offset = (scale / 2.0 + 1.0) as i32;

            for r in (offset..*nrows as i32 - offset).step_by(step as usize) {
                for c in (offset..*ncols as i32 - offset).step_by(step as usize) {
                    let q = self.classify_region(&r, &c, &scale, &pixels, &ldim);
                    if q > 0.0 {
                        self.detections.push(Detection(r, c, scale, q));
                    }
                    // break;
                }
                // break;
            }

            scale *= scale_factor;
        }
    }

    pub fn cluster_detections(&mut self, iou_threshold: f32) -> Vec<f32> {
        self.detections
            .sort_by(|a, b| (a.3).partial_cmp(&b.3).unwrap());
        let detections_lengh = self.detections.len();

        let mut assignments: Vec<u8> = vec![0; detections_lengh];
        let mut clusters: Vec<Cluster> = Vec::new();

        for (i, det) in self.detections.iter().enumerate() {
            if assignments[i] == 0 {
                let mut r: i32 = 0;
                let mut c: i32 = 0;
                let mut scale: f32 = 0.0;
                let mut q: f32 = 0.0;
                let mut n: f32 = 0.0;
                for j in i..detections_lengh {
                    let compare_det = &self.detections[j];
                    let Detection(r1, c1, scale1, q1) = compare_det;
                    if self.calculate_iou(det, compare_det) > iou_threshold {
                        assignments[j] = 1;
                        r += r1;
                        c += c1;
                        scale += scale1;
                        q += q1;
                        n += 1.0;
                    }
                }
                clusters.push(Cluster(r as f32 / n, c as f32 / n, scale / n as f32, q));
            }
        }

        let mut flattened_clusters: Vec<f32> = Vec::new();
        for cluster in clusters.iter() {
            let Cluster(r, c, scale, q) = cluster;
            flattened_clusters.push(*r);
            flattened_clusters.push(*c);
            flattened_clusters.push(*scale);
            flattened_clusters.push(*q);
        }
        flattened_clusters
    }

    fn classify_region(&self, r: &i32, c: &i32, scale: &f32, pixels: &Vec<u8>, ldim: &i32) -> f32 {
        let r = 256.0 * (*r as f32);
        let c = 256.0 * (*c as f32);
        let mut root = 0;
        let mut o: f32 = 0.0;

        for i in 0..self.ntrees {
            let mut idx = 1;

            for _ in 0..self.tdepth {
                let left_idx = ((r + self.tcodes[root + 4 * idx + 0] as i8 as f32 * scale) as i32
                    >> 8) * ldim
                    + ((c + self.tcodes[root + 4 * idx + 1] as i8 as f32 * scale) as i32 >> 8);
                let right_idx = ((r + self.tcodes[root + 4 * idx + 2] as i8 as f32 * scale) as i32
                    >> 8) * ldim
                    + ((c + self.tcodes[root + 4 * idx + 3] as i8 as f32 * scale) as i32 >> 8);
                idx *= 2;
                if pixels[left_idx as usize] <= pixels[right_idx as usize] {
                    idx += 1;
                }
            }

            // break;

            o = o + self.tpreds
                [self.tdepth_sqr as usize * i as usize + idx - self.tdepth_sqr as usize];
            if o <= self.thresh[i as usize] {
                return -1.0;
            }
            root += 4 * self.tdepth_sqr as usize;
        }

        o - self.thresh[(self.ntrees - 1) as usize]
    }

    fn calculate_iou(&self, det1: &Detection, det2: &Detection) -> f32 {
        let Detection(r1, c1, scale1, _) = det1;
        let Detection(r2, c2, scale2, _) = det2;
        let zero: f32 = 0.0;
        let overr = zero.max(
            (*r1 as f32 + scale1 / 2.0).min(*r2 as f32 + scale2 / 2.0)
                - (*r1 as f32 - scale1 / 2.0).max(*r2 as f32 - scale2 / 2.0),
        );
        let overc = zero.max(
            (*c1 as f32 + scale1 / 2.0).min(*c2 as f32 + scale2 / 2.0)
                - (*c1 as f32 - scale1 / 2.0).max(*c2 as f32 - scale2 / 2.0),
        );
        overr * overc / (scale1.powf(2.0) + scale2.powf(2.0) - overr * overc)
    }
}
