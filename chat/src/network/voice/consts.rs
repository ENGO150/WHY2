/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

//CONSTS (I HIGHLY RECOMMEND NOT CHANGING THOSE)
pub const SAMPLE_RATE: u32          = 48000;                                    //put some text here
pub const FRAME_MS: u32             = 20;                                       //LENGTH OF ONE FRAME
pub const FRAME_SIZE: usize         = (SAMPLE_RATE * FRAME_MS / 1000) as usize; //960 SAMPLES PER FRAME

pub const AGC_TARGET_RMS: f32       = 0.12;                                     //TARGET RMS AFTER NORMALIZATION (~-18dBFS)
pub const AGC_ATTACK: f32           = 0.25;                                     //ENVELOPE RISE (~80ms)
pub const AGC_RELEASE: f32          = 0.02;                                     //ENVELOPE FALL (~1s)
pub const AGC_GAIN_UP: f32          = 0.02;                                     //GAIN SLEW WHILE BOOSTING (~1s, SLOW = NO PUMPING)
pub const AGC_GAIN_DOWN: f32        = 0.3;                                      //GAIN SLEW WHILE CUTTING (~65ms, FAST = NO CLIPPING)
pub const AGC_MAX_GAIN: f32         = 40.0;                                     //MAX GAIN (+32dB, RESCUES QUIET MICS)
pub const AGC_MIN_GAIN: f32         = 0.05;                                     //MIN GAIN (-26dB, TAMES HOT MICS)
pub const LIMITER_KNEE: f32         = 0.7;                                      //SOFT CLIPPING STARTS AT THIS AMPLITUDE

pub const NOISE_FLOOR_ALPHA: f32    = 0.05;                                     //EMA SMOOTHING FACTOR WHILE THE FLOOR FALLS
pub const NOISE_FLOOR_RISE: f32     = 0.005;                                    //EMA SMOOTHING FACTOR WHILE IT RISES
pub const NOISE_OPEN_MULT: f32      = 3.5;                                      //OPEN TRESHOLD
pub const NOISE_CLOSE_MULT: f32     = 2.0;                                      //CLOSE TRESHOLD (HYSTERESIS)
pub const MIN_TRESHOLD_OPEN: f32    = 0.0008;                                   //HARD MINIMUM FOR OPEN
pub const MIN_TRESHOLD_CLOSE: f32   = 0.0004;                                   //HARD MINIMUM FOR CLOSE
pub const INITIAL_NOISE_FLOOR: f32  = 0.003;                                    //INIT

pub const HOLD_FRAMES: usize        = 10;                                       //~200ms HOLD TIME
pub const MIXING_TRESHOLD: f32      = 0.001;                                    //SPEAKER DETECTION NOISE TRESHOLD
pub const ACTIVITY_TRESHOLD: usize  = 100;                                      //SERVER ACTIVITY TIMER RESET TRESHOLD (~2000ms)
pub const SOUND_EFFECT_VOLUME: f32  = 0.1;                                      //VOLUME OF SOUND EFFECTS (10%)

pub const ACTIVITY_HOLD: usize      = (SAMPLE_RATE / 10) as usize;              //HOW LONG AFTER SPEAKING CLIENT BECOMES INACTIVE (~100ms)
pub const DISPLAY_HOLD: usize       = 200;                                      //ACTIVITY_HOLD BUT MS FOR DISPLAY WINDOW

pub const JITTER_BUFFER_SIZE: usize = 20;                                       //FRAME SIZE OF JITTER BUFFER

pub const GRID_WIDTH: usize         = 4;                                        //GRID WIDTH FOR VOICE PACKETS
pub const GRID_HEIGHT: usize        = 4;                                        //GRID HEIGHT FOR VOICE PACKETS

pub const HELLO_INTERVAL: u64       = 200;                                      //GAP BETWEEN TWO Hello ATTEMPTS (MS)
pub const HELLO_TIMEOUT: u64        = 5000;                                     //HOW LONG THE HANDSHAKE MAY GO UNANSWERED (MS)

pub const RECV_TIMEOUT: u64         = 200;                                      //UDP RECEIVE POLL TIMEOUT (MS)

pub const MAX_PACKET_RATE: f32      = (1000 / FRAME_MS) as f32 * 1.5;           //PACKETS PER SECOND ONE VOICE SESSION MAY SUSTAIN
pub const MAX_PACKET_BURST: f32     = MAX_PACKET_RATE / 2.0;                    //WHAT IT MAY SEND AT ONCE (~500ms OF JITTER AND REORDERING)
pub const SEND_CHANNEL_BOUND: usize = 8;                                        //AUDIO CALLBACK -> NETWORK TASK BUFFER
pub const VOLUME_MAX: u32           = 200;                                      //LOUDEST VOLUME SETTING (PERCENT)

pub const AEC_REFERENCE_CAPACITY: usize   = (SAMPLE_RATE * 2) as usize;         //REFERENCE RING
pub const AEC_SEARCH_RANGE: usize         = (SAMPLE_RATE * 3 / 10) as usize;    //FURTHEST THE PLAYBACK MAY LAG THE CAPTURE
pub const AEC_WINDOW: usize               = 4096;                               //SAMPLES EACH LAG IS SCORED OVER (~85ms)
pub const AEC_SEARCH_INTERVAL: usize      = (SAMPLE_RATE / 10) as usize;        //MINIMUM GAP BETWEEN TWO SEARCHES (~100ms)
pub const AEC_SEARCH_DECIMATION: usize    = 4;                                  //HOW FAR THE SEARCH DECIMATES
pub const AEC_PEAK_SIGMA: f32             = 6.0;                                //HOW FAR THE PEAK MUST STAND ABOVE THE REST
pub const AEC_MIN_ENERGY: f32             = 0.05;                               //REFERENCE ENERGY BELOW THIS IS SILENCE, NOT A SIGNAL
pub const AEC_ADAPT_RATIO: f32            = 0.5;                                //HOW MUCH OF THE CAPTURE MUST BE OUR ECHO
pub const AEC_MIN_GAIN: f32               = 0.05;                               //QUIETEST PLAUSIBLE ECHO (-26dB)
pub const AEC_MAX_GAIN: f32               = 2.0;                                //LOUDEST PLAUSIBLE ECHO (+6dB)
pub const AEC_TAPS: usize                 = 256;                                //FILTER LENGTH (~5ms AROUND THE ESTIMATE)
pub const AEC_LEAD_TAPS: usize            = 64;                                 //HOW MUCH OF IT SITS AHEAD OF THE ESTIMATE (~1ms)
pub const AEC_STEP: f32                   = 0.0001;                              //NLMS STEP SIZE
pub const AEC_EPSILON: f32                = 1e-6;                               //NLMS REGULARIZATION (NEVER DIVIDE BY A SILENT WINDOW)
pub const AEC_SCORE_WINDOW: usize         = SAMPLE_RATE as usize;               //HOW OFTEN THE FILTER HAS TO JUSTIFY ITSELF (~1s)
pub const AEC_ROLLBACK_MARGIN: f32        = 3.0;                                //dB WORSE THAN THE BEST BEFORE A ROLLBACK
pub const AEC_ROLLBACK_DECAY: f32         = 1.0;                                //dB THE STANDARD TO BEAT FORGETS EACH WINDOW
pub const AEC_ROLLBACK_LIMIT: usize       = 3;                                  //ROLLBACKS IN A ROW THAT DO NOT RESCUE IT
pub const AEC_SCORE_FLOOR: f32            = 0.05;                               //TOO LITTLE WENT THROUGH THAT WINDOW TO JUDGE IT
pub const AEC_HISTORY: usize              = AEC_SEARCH_RANGE + AEC_WINDOW;      //HOW MUCH OF EACH SIDE THE SEARCH NEEDS
