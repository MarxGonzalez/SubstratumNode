// Copyright (c) 2017-2019, Substratum LLC (https://substratum.net) and/or its affiliates. All rights reserved.

import {TestBed} from '@angular/core/testing';
import {MainService} from './main.service';
import {ElectronService} from './electron.service';
import {func, matchers, object, reset, verify, when} from 'testdouble';
import {NodeConfiguration} from './node-configuration';
import {ConfigService} from './config.service';

describe('MainService', () => {
  let stubElectronService;
  let stubClipboard;
  let mockWriteText;
  let mockSendSync;
  let service: MainService;
  let mockConfigService;
  let mockGetConfig;
  let mockOn;
  let nodeStatusListener;
  let nodeDescriptorListener;

  beforeEach(() => {
    mockSendSync = func('sendSync');
    mockGetConfig = func('getConfig');
    mockWriteText = func('writeText');
    mockOn = func('on');
    nodeStatusListener = matchers.captor();
    nodeDescriptorListener = matchers.captor();
    stubClipboard = {
      writeText: mockWriteText
    };
    stubElectronService = {
      ipcRenderer: {
        on: mockOn,
        sendSync: mockSendSync,
      },
      clipboard: stubClipboard
    };
    mockConfigService = object(['getConfig']);
    mockConfigService = {
      getConfig: mockGetConfig
    };
    TestBed.configureTestingModule({
      providers: [
        MainService,
        {provide: ElectronService, useValue: stubElectronService},
        {provide: ConfigService, useValue: mockConfigService}
      ]
    });
    service = TestBed.get(MainService);
  });

  afterEach(() => {
    reset();
  });

  it('should be created', () => {
    expect(service).toBeTruthy();
  });

  it('creates listeners', () => {
    verify(mockOn('node-status', nodeStatusListener.capture()));
    verify(mockOn('node-descriptor', nodeDescriptorListener.capture()));

    nodeStatusListener.value('', 'the status');
    nodeDescriptorListener.value('', 'the descriptor');

    service.nodeStatus.subscribe(status => {
      expect(status).toEqual('the status');
    });
    service.nodeDescriptor.subscribe(descriptor => {
      expect(descriptor).toEqual('the descriptor');
    });
  });

  describe('user actions', () => {
    beforeEach(() => {
      when(mockGetConfig()).thenReturn(new NodeConfiguration());
      when(mockSendSync('change-node-state', 'turn-off', matchers.anything())).thenReturn('Off');
      when(mockSendSync('change-node-state', 'serve', matchers.anything())).thenReturn('Serving');
      when(mockSendSync('change-node-state', 'consume', matchers.anything())).thenReturn('Consuming');
      when(mockSendSync('ip-lookup')).thenReturn('4.3.2.1');
    });

    it('tells the main to turn off', () => {
      service.turnOff().subscribe((v) => {
        expect(v).toBe('Off');
      });
    });

    it('tells the main to switch to serving', () => {
      service.serve().subscribe((v) => {
        expect(v).toBe('Serving');
      });
    });

    it('tells the main to switch to consuming', () => {
      service.consume().subscribe((v) => {
        expect(v).toBe('Consuming');
      });
    });

    it('looks up the ip address', () => {
      service.lookupIp().subscribe(result => expect(result).toBe('4.3.2.1'));
    });

    describe('when configuration exists', () => {
      const nodeConfig: NodeConfiguration = {ip: 'fake'};
      beforeEach(() => {
        when(mockGetConfig()).thenReturn(nodeConfig);
        service.serve().subscribe((_) => _);
        service.consume().subscribe((_) => _);
      });

      it('is included in serving', () => {
        verify(mockSendSync('change-node-state', 'serve', nodeConfig));
      });

      it('is included in consuming', () => {
        verify(mockSendSync('change-node-state', 'consume', nodeConfig));
      });
    });
  });

  describe('copy', () => {
    beforeEach(() => {
      service.copyToClipboard('this is not a dance');
    });

    it('writes to the clipboard', () => {
      verify(mockWriteText('this is not a dance'));
    });
  });
});
