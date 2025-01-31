// Licensed under the Apache License, Version 2.0 or the MIT License.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright Tock Contributors 2022.

//! ADC driver for the nRF52. Uses the SAADC peripheral.

use kernel::hil;
use kernel::utilities::cells::{OptionalCell, VolatileCell};
use kernel::utilities::registers::interfaces::{Readable, Writeable};
use kernel::utilities::registers::{register_bitfields, ReadOnly, ReadWrite, WriteOnly};
use kernel::utilities::StaticRef;
use kernel::ErrorCode;

#[repr(C)]
struct AdcRegisters {
    /// Start the ADC and prepare the result buffer in RAM
    tasks_start: WriteOnly<u32, TASK::Register>,
    /// Take one ADC sample, if scan is enabled all channels are sampled
    tasks_sample: WriteOnly<u32, TASK::Register>,
    /// Stop the ADC and terminate any on-going conversion
    tasks_stop: WriteOnly<u32, TASK::Register>,
    /// Starts offset auto-calibration
    tasks_calibrateoffset: WriteOnly<u32, TASK::Register>,
    _reserved0: [u8; 240],
    /// The ADC has started
    events_started: ReadWrite<u32, EVENT::Register>,
    /// The ADC has filled up the Result buffer
    events_end: ReadWrite<u32, EVENT::Register>,
    /// A conversion task has been completed. Depending on the mode, multiple conversion
    events_done: ReadWrite<u32, EVENT::Register>,
    /// A result is ready to get transferred to RAM
    events_resultdone: ReadWrite<u32, EVENT::Register>,
    /// Calibration is complete
    events_calibratedone: ReadWrite<u32, EVENT::Register>,
    /// The ADC has stopped
    events_stopped: ReadWrite<u32, EVENT::Register>,
    /// Last result is equal or above `CH[X].LIMIT`
    events_ch: [AdcEventChRegisters; 8],
    _reserved1: [u8; 424],
    /// Enable or disable interrupt
    inten: ReadWrite<u32, INTEN::Register>,
    /// Enable interrupt
    intenset: ReadWrite<u32, INTEN::Register>,
    /// Disable interrupt
    intenclr: ReadWrite<u32, INTEN::Register>,
    _reserved2: [u8; 244],
    /// Status
    status: ReadOnly<u32>,
    _reserved3: [u8; 252],
    /// Enable or disable ADC
    enable: ReadWrite<u32, ENABLE::Register>,
    _reserved4: [u8; 12],
    ch: [AdcChRegisters; 8],
    _reserved5: [u8; 96],
    /// Resolution configuration
    resolution: ReadWrite<u32, RESOLUTION::Register>,
    /// Oversampling configuration. OVERSAMPLE should not be combined with SCAN. The RES
    oversample: ReadWrite<u32>,
    /// Controls normal or continuous sample rate
    samplerate: ReadWrite<u32, SAMPLERATE::Register>,
    _reserved6: [u8; 48],
    /// Pointer to store samples to
    result_ptr: VolatileCell<*const u16>,
    /// Number of 16 bit samples to save in RAM
    result_maxcnt: ReadWrite<u32, RESULT_MAXCNT::Register>,
    /// Number of 16 bit samples recorded to RAM
    result_amount: ReadWrite<u32, RESULT_AMOUNT::Register>,
}

#[repr(C)]
struct AdcEventChRegisters {
    limith: ReadWrite<u32, EVENT::Register>,
    limitl: ReadWrite<u32, EVENT::Register>,
}

#[repr(C)]
struct AdcChRegisters {
    pselp: ReadWrite<u32, PSEL::Register>,
    pseln: ReadWrite<u32, PSEL::Register>,
    config: ReadWrite<u32, CONFIG::Register>,
    limit: ReadWrite<u32, LIMIT::Register>,
}

register_bitfields![u32,
    INTEN [
        /// Enable or disable interrupt on EVENTS_STARTED event
        STARTED 0,
        /// Enable or disable interrupt on EVENTS_END event
        END 1,
        /// Enable or disable interrupt on EVENTS_DONE event
        DONE 2,
        /// Enable or disable interrupt on EVENTS_RESULTDONE event
        RESULTDONE 3,
        /// Enable or disable interrupt on EVENTS_CALIBRATEDONE event
        CALIBRATEDONE 4,
        /// Enable or disable interrupt on EVENTS_STOPPED event
        STOPPED 5,
        /// Enable or disable interrupt on EVENTS_CH[0].LIMITH event
        CH0LIMITH 6,
        /// Enable or disable interrupt on EVENTS_CH[0].LIMITL event
        CH0LIMITL 7,
        /// Enable or disable interrupt on EVENTS_CH[1].LIMITH event
        CH1LIMITH 8,
        /// Enable or disable interrupt on EVENTS_CH[1].LIMITL event
        CH1LIMITL 9,
        /// Enable or disable interrupt on EVENTS_CH[2].LIMITH event
        CH2LIMITH 10,
        /// Enable or disable interrupt on EVENTS_CH[2].LIMITL event
        CH2LIMITL 11,
        /// Enable or disable interrupt on EVENTS_CH[3].LIMITH event
        CH3LIMITH 12,
        /// Enable or disable interrupt on EVENTS_CH[3].LIMITL event
        CH3LIMITL 13,
        /// Enable or disable interrupt on EVENTS_CH[4].LIMITH event
        CH4LIMITH 14,
        /// Enable or disable interrupt on EVENTS_CH[4].LIMITL event
        CH4LIMITL 15,
        /// Enable or disable interrupt on EVENTS_CH[5].LIMITH event
        CH5LIMITH 16,
        /// Enable or disable interrupt on EVENTS_CH[5].LIMITL event
        CH5LIMITL 17,
        /// Enable or disable interrupt on EVENTS_CH[6].LIMITH event
        CH6LIMITH 18,
        /// Enable or disable interrupt on EVENTS_CH[6].LIMITL event
        CH6LIMITL 19,
        /// Enable or disable interrupt on EVENTS_CH[7].LIMITH event
        CH7LIMITH 20,
        /// Enable or disable interrupt on EVENTS_CH[7].LIMITL event
        CH7LIMITL 21
    ],
    ENABLE [
        ENABLE 0
    ],
    SAMPLERATE [
        /// Capture and compare value. Sample rate is 16 MHz/CC
        CC OFFSET(0) NUMBITS(11) [],
        /// Select mode for sample rate control
        MODE OFFSET(12) NUMBITS(1) [
            /// Rate is controlled from SAMPLE task
            Task = 0,
            /// Rate is controlled from local timer (use CC to control the rate)
            Timers = 1
        ]
    ],
    EVENT [
        EVENT 0
    ],
    TASK [
        TASK 0
    ],
    PSEL [
        PSEL OFFSET(0) NUMBITS(5) [
            NotConnected = 0,
            AnalogInput0 = 1,
            AnalogInput1 = 2,
            AnalogInput2 = 3,
            AnalogInput3 = 4,
            AnalogInput4 = 5,
            AnalogInput5 = 6,
            AnalogInput6 = 7,
            AnalogInput7 = 8,
            VDD = 9,
            VDDHDIV5 = 0xD
        ]
    ],
    CONFIG [
        RESP OFFSET(0) NUMBITS(2) [
            Bypass = 0,
            Pulldown = 1,
            Pullup = 2,
            VDD1_2 = 3
        ],
        RESN OFFSET(4) NUMBITS(2) [
            Bypass = 0,
            Pulldown = 1,
            Pullup = 2,
            VDD1_2 = 3
        ],
        GAIN OFFSET(8) NUMBITS(3) [
            Gain1_6 = 0,
            Gain1_5 = 1,
            Gain1_4 = 2,
            Gain1_3 = 3,
            Gain1_2 = 4,
            Gain1 = 5,
            Gain2 = 6,
            Gain4 = 7
        ],
        REFSEL OFFSET(12) NUMBITS(1) [
            Internal = 0,
            VDD1_4 = 1
        ],
        TACQ OFFSET(16) NUMBITS(3) [
            us3 = 0,
            us5 = 1,
            us10 = 2,
            us15 = 3,
            us20 = 4,
            us40 = 5
        ],
        MODE OFFSET(20) NUMBITS(1) [
            SE = 0,
            Diff = 1
        ],
        BURST OFFSET(24) NUMBITS(1) [
            Disable = 0,
            Enable = 1
        ]
    ],
    LIMIT [
        LOW OFFSET(0) NUMBITS(16) [],
        HIGH OFFSET(16) NUMBITS(16) []
    ],
    RESOLUTION [
        VAL OFFSET(0) NUMBITS(3) [
            bit8 = 0,
            bit10 = 1,
            bit12 = 2,
            bit14 = 3
        ]
    ],
    RESULT_MAXCNT [
        MAXCNT OFFSET(0) NUMBITS(16) []
    ],
    RESULT_AMOUNT [
        AMOUNT OFFSET(0) NUMBITS(16) []
    ]
];

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AdcChannel {
    AnalogInput0 = 1,
    AnalogInput1 = 2,
    AnalogInput2 = 3,
    AnalogInput3 = 4,
    AnalogInput4 = 5,
    AnalogInput5 = 6,
    AnalogInput6 = 7,
    AnalogInput7 = 8,
    VDD = 9,
    VDDHDIV5 = 0xD,
}

const SAADC_BASE: StaticRef<AdcRegisters> =
    unsafe { StaticRef::new(0x40007000 as *const AdcRegisters) };

// Buffer to save completed sample to.
static mut SAMPLE: [u16; 1] = [0; 1];

#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum AdcChannelGain {
    Gain1_6 = 0,
    Gain1_5 = 1,
    Gain1_4 = 2,
    Gain1_3 = 3,
    Gain1_2 = 4,
    Gain1 = 5,
    Gain2 = 6,
    Gain4 = 7,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum AdcChannelResistor {
    Bypass = 0,
    Pulldown = 1,
    Pullup = 2,
    VDD1_2 = 3,
}

#[allow(non_camel_case_types)]
#[repr(u8)]
#[derive(Clone, Copy, Debug)]
pub enum AdcChannelSamplingTime {
    us3 = 0,
    us5 = 1,
    us10 = 2,
    us15 = 3,
    us20 = 4,
    us40 = 5,
}

#[derive(Copy, Clone, Debug)]
pub struct AdcChannelSetup {
    channel: AdcChannel,
    gain: AdcChannelGain,
    resp: AdcChannelResistor,
    resn: AdcChannelResistor,
    sampling_time: AdcChannelSamplingTime,
}

impl PartialEq for AdcChannelSetup {
    fn eq(&self, other: &Self) -> bool {
        self.channel == other.channel
    }
}

impl AdcChannelSetup {
    pub fn new(channel: AdcChannel) -> AdcChannelSetup {
        AdcChannelSetup {
            channel,
            gain: AdcChannelGain::Gain1_4,
            resp: AdcChannelResistor::Bypass,
            resn: AdcChannelResistor::Pulldown,
            sampling_time: AdcChannelSamplingTime::us10,
        }
    }

    pub fn setup(
        channel: AdcChannel,
        gain: AdcChannelGain,
        resp: AdcChannelResistor,
        resn: AdcChannelResistor,
        sampling_time: AdcChannelSamplingTime,
    ) -> AdcChannelSetup {
        AdcChannelSetup {
            channel,
            gain,
            resp,
            resn,
            sampling_time,
        }
    }
}

pub struct Adc {
    registers: StaticRef<AdcRegisters>,
    client: OptionalCell<&'static dyn hil::adc::Client>,
}

impl Adc {
    pub const fn new() -> Self {
        Self {
            registers: SAADC_BASE,
            client: OptionalCell::empty(),
        }
    }

    pub fn calibrate(&self) {
        // Enable the ADC
        self.registers.enable.write(ENABLE::ENABLE::SET);
        self.registers.inten.write(INTEN::CALIBRATEDONE::SET);
        self.registers.tasks_calibrateoffset.write(TASK::TASK::SET);
    }

    pub fn handle_interrupt(&self) {
        // Determine what event occurred.
        if self.registers.events_calibratedone.is_set(EVENT::EVENT) {
            self.registers
                .events_calibratedone
                .write(EVENT::EVENT::CLEAR);
            self.registers.enable.write(ENABLE::ENABLE::CLEAR);
        } else if self.registers.events_started.is_set(EVENT::EVENT) {
            self.registers.events_started.write(EVENT::EVENT::CLEAR);
            // ADC has started, now issue the sample.
            self.registers.tasks_sample.write(TASK::TASK::SET);
        } else if self.registers.events_end.is_set(EVENT::EVENT) {
            self.registers.events_end.write(EVENT::EVENT::CLEAR);
            // Reading finished. Turn off the ADC.
            self.registers.tasks_stop.write(TASK::TASK::SET);
        } else if self.registers.events_stopped.is_set(EVENT::EVENT) {
            self.registers.events_stopped.write(EVENT::EVENT::CLEAR);
            // ADC is stopped. Disable and return value.
            self.registers.enable.write(ENABLE::ENABLE::CLEAR);

            let val = unsafe { SAMPLE[0] as i16 };
            self.client.map(|client| {
                // shift left to meet the ADC HIL requirement
                client.sample_ready(if val < 0 { 0 } else { val << 4 } as u16);
            });
        }
    }
}

/// Implements an ADC capable reading ADC samples on any channel.
impl hil::adc::Adc for Adc {
    type Channel = AdcChannelSetup;

    fn sample(&self, channel: &Self::Channel) -> Result<(), ErrorCode> {
        // Positive goes to the channel passed in, negative not connected.
        self.registers.ch[0]
            .pselp
            .write(PSEL::PSEL.val(channel.channel as u32));
        self.registers.ch[0].pseln.write(PSEL::PSEL::NotConnected);

        // Configure the ADC for a single read.
        self.registers.ch[0].config.write(
            CONFIG::GAIN.val(channel.gain as u32)
                + CONFIG::REFSEL::VDD1_4
                + CONFIG::TACQ.val(channel.sampling_time as u32)
                + CONFIG::RESP.val(channel.resp as u32)
                + CONFIG::RESN.val(channel.resn as u32)
                + CONFIG::MODE::SE,
        );

        // Set max resolution (with oversampling).
        self.registers.resolution.write(RESOLUTION::VAL::bit12);

        // Do one measurement.
        self.registers
            .result_maxcnt
            .write(RESULT_MAXCNT::MAXCNT.val(1));
        // Where to put the reading.
        unsafe {
            self.registers.result_ptr.set(SAMPLE.as_ptr());
        }

        // No automatic sampling, will trigger manually.
        self.registers.samplerate.write(SAMPLERATE::MODE::Task);

        // Enable the ADC
        self.registers.enable.write(ENABLE::ENABLE::SET);

        // Enable started, sample end, and stopped interrupts.
        self.registers
            .inten
            .write(INTEN::STARTED::SET + INTEN::END::SET + INTEN::STOPPED::SET);

        // Start the SAADC and wait for the started interrupt.
        self.registers.tasks_start.write(TASK::TASK::SET);

        Ok(())
    }

    fn sample_continuous(
        &self,
        _channel: &Self::Channel,
        _frequency: u32,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::FAIL)
    }

    fn stop_sampling(&self) -> Result<(), ErrorCode> {
        Err(ErrorCode::FAIL)
    }

    fn get_resolution_bits(&self) -> usize {
        12
    }

    fn get_voltage_reference_mv(&self) -> Option<usize> {
        Some(3300)
    }

    fn set_client(&self, client: &'static dyn hil::adc::Client) {
        self.client.set(client);
    }
}
